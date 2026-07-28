use crate::pattern::{Addr, PRECEDENCE, Pattern, PatternError, Rank};

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
