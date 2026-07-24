#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Signature {
    pub status: SignatureStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SignatureStatus {
    #[default]
    None,
    Good {
        signer: String,
        key_id: String,
    },
    Unknown {
        key_id: String,
    },
    Bad {
        key_id: String,
    },
}

impl Signature {
    pub fn none() -> Signature {
        Signature::default()
    }

    pub fn from_status(status: SignatureStatus) -> Signature {
        Signature { status }
    }

    pub fn is_signed(&self) -> bool {
        !matches!(self.status, SignatureStatus::None)
    }

    /// The pager header line for this outcome, or `None` when the
    /// message carries no signature and nothing should be shown.
    pub fn header_line(&self) -> Option<String> {
        self.status.header_line()
    }
}

impl SignatureStatus {
    pub fn header_line(&self) -> Option<String> {
        match self {
            SignatureStatus::None => None,
            SignatureStatus::Good { signer, key_id } => Some(format!(
                "Good signature from {signer} (0x{key_id})"
            )),
            SignatureStatus::Unknown { key_id } => Some(format!(
                "Unknown signature from key 0x{key_id} \
                 (not in keyring)"
            )),
            SignatureStatus::Bad { key_id } => {
                Some(format!("BAD signature (0x{key_id})"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Signature, SignatureStatus};

    #[test]
    fn header_lines_read_unmistakably_per_status() {
        let cases = [
            (SignatureStatus::None, None),
            (
                SignatureStatus::Good {
                    signer: "Alice <alice@example.com>".to_string(),
                    key_id: "1A2B3C4D5E6F7A8B".to_string(),
                },
                Some(
                    "Good signature from Alice \
                     <alice@example.com> (0x1A2B3C4D5E6F7A8B)",
                ),
            ),
            (
                SignatureStatus::Unknown {
                    key_id: "DEADBEEFDEADBEEF".to_string(),
                },
                Some(
                    "Unknown signature from key \
                     0xDEADBEEFDEADBEEF (not in keyring)",
                ),
            ),
            (
                SignatureStatus::Bad {
                    key_id: "0BADC0DE0BADC0DE".to_string(),
                },
                Some("BAD signature (0x0BADC0DE0BADC0DE)"),
            ),
        ];
        for (status, expected) in cases {
            let signature = Signature::from_status(status);
            assert_eq!(signature.header_line().as_deref(), expected,);
        }
    }

    #[test]
    fn none_is_not_signed() {
        assert!(!Signature::none().is_signed());
        assert!(
            Signature::from_status(SignatureStatus::Unknown {
                key_id: "AAAA".to_string(),
            })
            .is_signed()
        );
    }
}
