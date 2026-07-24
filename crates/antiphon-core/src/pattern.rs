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
pub(crate) enum Rank {
    Literal,
    LocalGlob,
    CatchAll,
}

pub(crate) const PRECEDENCE: [Rank; 3] =
    [Rank::Literal, Rank::LocalGlob, Rank::CatchAll];

impl Pattern {
    pub(crate) fn rank(&self) -> Rank {
        match self {
            Self::Literal { .. } => Rank::Literal,
            Self::LocalGlob { .. } => Rank::LocalGlob,
            Self::CatchAll { .. } => Rank::CatchAll,
        }
    }

    pub(crate) fn matches(&self, addr: &Addr) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
