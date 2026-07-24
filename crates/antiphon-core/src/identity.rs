use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    Literal {
        local: String,
        domain: String,
    },
    LocalGlob {
        prefix: String,
        suffix: String,
        domain: String,
    },
    CatchAll {
        domain: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rank {
    Literal,
    LocalGlob,
    CatchAll,
}

const PRECEDENCE: [Rank; 3] =
    [Rank::Literal, Rank::LocalGlob, Rank::CatchAll];

impl Pattern {
    fn rank(&self) -> Rank {
        match self {
            Self::Literal { .. } => Rank::Literal,
            Self::LocalGlob { .. } => Rank::LocalGlob,
            Self::CatchAll { .. } => Rank::CatchAll,
        }
    }

    fn matches(&self, addr: &Addr) -> bool {
        match self {
            Self::Literal { local, domain } => {
                *local == addr.local && *domain == addr.domain
            }
            Self::LocalGlob {
                prefix,
                suffix,
                domain,
            } => {
                *domain == addr.domain
                    && addr.local.len() >= prefix.len() + suffix.len()
                    && addr.local.starts_with(prefix.as_str())
                    && addr.local.ends_with(suffix.as_str())
            }
            Self::CatchAll { domain } => *domain == addr.domain,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    NotAnAddress { pattern: String },
    StarInDomain { pattern: String },
    MultipleStars { pattern: String },
}

impl fmt::Display for PatternError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAddress { pattern } => write!(
                out,
                "identity match pattern `{pattern}` is not a \
                 local@domain address",
            ),
            Self::StarInDomain { pattern } => write!(
                out,
                "identity match pattern `{pattern}` puts `*` in \
                 the domain; only the local part may glob",
            ),
            Self::MultipleStars { pattern } => write!(
                out,
                "identity match pattern `{pattern}` has more \
                 than one `*`",
            ),
        }
    }
}

impl std::error::Error for PatternError {}

impl FromStr for Pattern {
    type Err = PatternError;

    fn from_str(text: &str) -> Result<Self, PatternError> {
        let Some((local, domain)) = text.rsplit_once('@') else {
            return Err(PatternError::NotAnAddress {
                pattern: text.to_owned(),
            });
        };
        if local.is_empty() || domain.is_empty() {
            return Err(PatternError::NotAnAddress {
                pattern: text.to_owned(),
            });
        }
        if domain.contains('*') {
            return Err(PatternError::StarInDomain {
                pattern: text.to_owned(),
            });
        }
        if local.matches('*').count() > 1 {
            return Err(PatternError::MultipleStars {
                pattern: text.to_owned(),
            });
        }
        let domain = domain.to_lowercase();
        if local == "*" {
            return Ok(Self::CatchAll { domain });
        }
        match local.split_once('*') {
            None => Ok(Self::Literal {
                local: local.to_lowercase(),
                domain,
            }),
            Some((prefix, suffix)) => Ok(Self::LocalGlob {
                prefix: prefix.to_lowercase(),
                suffix: suffix.to_lowercase(),
                domain,
            }),
        }
    }
}

pub fn validate_patterns(
    patterns: &[impl AsRef<str>],
) -> Result<(), PatternError> {
    for pattern in patterns {
        pattern.as_ref().parse::<Pattern>()?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Addr {
    original: String,
    local: String,
    domain: String,
}

impl Addr {
    pub fn new(text: &str) -> Self {
        let (local, domain) =
            text.rsplit_once('@').unwrap_or((text, ""));
        Self {
            original: text.to_owned(),
            local: local.to_lowercase(),
            domain: domain.to_lowercase(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.original
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedIdentity {
    pub address: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub patterns: Vec<Pattern>,
}

impl ParsedIdentity {
    pub fn new(
        address: &str,
        name: Option<&str>,
        signature: Option<&str>,
        patterns: &[impl AsRef<str>],
    ) -> Result<Self, PatternError> {
        let patterns = patterns
            .iter()
            .map(|pattern| pattern.as_ref().parse())
            .collect::<Result<_, _>>()?;
        Ok(Self {
            address: address.to_owned(),
            name: name.map(str::to_owned),
            signature: signature.map(str::to_owned),
            patterns,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resolved<'a> {
    pub identity: &'a ParsedIdentity,
    pub from: String,
}

pub fn reply_identity<'a>(
    identities: &'a [ParsedIdentity],
    delivered_to: &[Addr],
) -> Option<Resolved<'a>> {
    for rank in PRECEDENCE {
        for identity in identities {
            for addr in delivered_to {
                if !matches_at(identity, addr, rank) {
                    continue;
                }
                return Some(Resolved {
                    identity,
                    from: addr.as_str().to_owned(),
                });
            }
        }
    }
    None
}

fn matches_at(
    identity: &ParsedIdentity,
    addr: &Addr,
    rank: Rank,
) -> bool {
    identity
        .patterns
        .iter()
        .any(|pattern| pattern.rank() == rank && pattern.matches(addr))
}

pub fn compose_identity(
    identities: &[ParsedIdentity],
) -> Option<&ParsedIdentity> {
    identities.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(
        address: &str,
        name: Option<&str>,
        patterns: &[&str],
    ) -> ParsedIdentity {
        ParsedIdentity::new(address, name, None, patterns)
            .expect("test patterns parse")
    }

    fn addrs(texts: &[&str]) -> Vec<Addr> {
        texts.iter().map(|text| Addr::new(text)).collect()
    }

    fn tiered() -> Vec<ParsedIdentity> {
        vec![
            identity(
                "catch@example.com",
                Some("Catch"),
                &["*@example.com"],
            ),
            identity(
                "plus@example.com",
                Some("Plus"),
                &["quin+*@example.com"],
            ),
            identity(
                "lit@example.com",
                Some("Lit"),
                &["quin@example.com"],
            ),
        ]
    }

    #[test]
    fn most_specific_pattern_wins() {
        let identities = tiered();
        let cases: &[(&[&str], &str, &str)] = &[
            (&["quin@example.com"], "Lit", "quin@example.com"),
            (
                &["quin+shop@example.com"],
                "Plus",
                "quin+shop@example.com",
            ),
            (&["team@example.com"], "Catch", "team@example.com"),
            (
                &["team@example.com", "quin@example.com"],
                "Lit",
                "quin@example.com",
            ),
        ];
        for (delivered, name, from) in cases {
            let resolved =
                reply_identity(&identities, &addrs(delivered))
                    .expect("should resolve");
            assert_eq!(
                resolved.identity.name.as_deref(),
                Some(*name),
                "delivered {delivered:?}",
            );
            assert_eq!(resolved.from, *from);
        }
    }

    #[test]
    fn pattern_hits_reply_as_the_delivered_address() {
        let identities = [identity(
            "quin@example.com",
            Some("Q"),
            &["*@quin.example.com", "quin+*@example.com"],
        )];
        let cases =
            ["Shop-Orders@Quin.Example.Com", "QUIN+News@EXAMPLE.COM"];
        for delivered in cases {
            let resolved =
                reply_identity(&identities, &addrs(&[delivered]))
                    .expect("should resolve");
            assert_eq!(resolved.from, delivered);
            assert_eq!(resolved.identity.name.as_deref(), Some("Q"),);
        }
    }

    #[test]
    fn literal_matching_ignores_case_keeps_spelling() {
        let identities =
            [identity("quin@example.com", None, &["quin@example.com"])];
        let resolved =
            reply_identity(&identities, &addrs(&["QUIN@Example.COM"]))
                .expect("should resolve");
        assert_eq!(resolved.from, "QUIN@Example.COM");
    }

    #[test]
    fn plus_pattern_does_not_match_the_bare_address() {
        let identities = [identity(
            "quin@example.com",
            None,
            &["quin+*@example.com"],
        )];
        let delivered = addrs(&["quin@example.com"]);
        assert_eq!(reply_identity(&identities, &delivered), None);
    }

    #[test]
    fn unmatched_delivery_returns_none() {
        let identities = tiered();
        assert_eq!(
            reply_identity(
                &identities,
                &addrs(&["quin@elsewhere.org"]),
            ),
            None,
        );
        assert_eq!(reply_identity(&identities, &[]), None);
    }

    #[test]
    fn config_order_breaks_ties() {
        let identities = [
            identity(
                "first@example.com",
                Some("First"),
                &["*@example.com"],
            ),
            identity(
                "second@example.com",
                Some("Second"),
                &["*@example.com"],
            ),
        ];
        let resolved =
            reply_identity(&identities, &addrs(&["team@example.com"]))
                .expect("should resolve");
        assert_eq!(resolved.identity.name.as_deref(), Some("First"),);
    }

    #[test]
    fn compose_uses_the_first_identity() {
        let identities = tiered();
        let first =
            compose_identity(&identities).expect("should pick one");
        assert_eq!(first.address, "catch@example.com");
        assert_eq!(compose_identity(&[]), None);
    }

    #[test]
    fn good_patterns_parse_to_their_variants() {
        let cases: &[(&str, Pattern)] = &[
            (
                "Quin@Example.com",
                Pattern::Literal {
                    local: "quin".to_owned(),
                    domain: "example.com".to_owned(),
                },
            ),
            (
                "*@quin.example.com",
                Pattern::CatchAll {
                    domain: "quin.example.com".to_owned(),
                },
            ),
            (
                "quin+*@example.com",
                Pattern::LocalGlob {
                    prefix: "quin+".to_owned(),
                    suffix: String::new(),
                    domain: "example.com".to_owned(),
                },
            ),
            (
                "quin*box@example.com",
                Pattern::LocalGlob {
                    prefix: "quin".to_owned(),
                    suffix: "box".to_owned(),
                    domain: "example.com".to_owned(),
                },
            ),
        ];
        for (text, expected) in cases {
            let parsed: Pattern = text.parse().expect("should parse");
            assert_eq!(parsed, *expected, "pattern {text}");
        }
    }

    #[test]
    fn validate_rejects_bad_patterns_naming_them() {
        let bad = |pattern: &str| pattern.to_owned();
        let cases = [
            (
                "*@*.example.com",
                PatternError::StarInDomain {
                    pattern: bad("*@*.example.com"),
                },
            ),
            (
                "a*b*@example.com",
                PatternError::MultipleStars {
                    pattern: bad("a*b*@example.com"),
                },
            ),
            (
                "quin",
                PatternError::NotAnAddress {
                    pattern: bad("quin"),
                },
            ),
            (
                "@example.com",
                PatternError::NotAnAddress {
                    pattern: bad("@example.com"),
                },
            ),
            (
                "quin@",
                PatternError::NotAnAddress {
                    pattern: bad("quin@"),
                },
            ),
        ];
        for (pattern, expected) in cases {
            let error = validate_patterns(&[pattern])
                .expect_err("should reject");
            assert_eq!(error, expected);
            assert!(
                error.to_string().contains(pattern),
                "message names {pattern}",
            );
        }
    }

    #[test]
    fn config_identity_shape_round_trips() {
        let config = antiphon_config::Identity {
            address: "quin@example.com".to_owned(),
            name: Some("Q".to_owned()),
            signature: Some("personal".to_owned()),
            matches: vec![
                "quin@example.com".to_owned(),
                "*@quin.example.com".to_owned(),
                "quin+*@example.com".to_owned(),
            ],
            pgp_sign: false,
            pgp_key: None,
        };
        let parsed = ParsedIdentity::new(
            &config.address,
            config.name.as_deref(),
            config.signature.as_deref(),
            &config.matches,
        )
        .expect("config patterns parse");
        let resolved = reply_identity(
            std::slice::from_ref(&parsed),
            &addrs(&["shop-orders@quin.example.com"]),
        )
        .expect("should resolve");
        assert_eq!(resolved.from, "shop-orders@quin.example.com");
        assert_eq!(resolved.identity.name.as_deref(), Some("Q"));
        assert_eq!(
            resolved.identity.signature.as_deref(),
            Some("personal"),
        );
    }
}
