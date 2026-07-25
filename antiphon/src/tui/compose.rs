use antiphon_render::{Draft, build_message};
use antiphon_store::Envelope;
use ratatui::crossterm::event::KeyEvent;

use super::crypto::{ComposeCrypto, PgpPlan};
use super::headers::{HeaderFields, HeadersOutcome};
use super::identity::ComposeIdentity;
use super::prefill::DraftFields;

/// One entry the From field can cycle to: a configured
/// identity and the account it sends through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IdentityChoice {
    pub account: String,
    pub identity: ComposeIdentity,
}

/// A compose in flight, across the fields stage, the body
/// editor and the review screen.
pub(super) struct ComposeState {
    pub choices: Vec<IdentityChoice>,
    pub chosen: usize,
    pub fields: HeaderFields,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub sign_override: Option<bool>,
    pub encrypt_override: Option<bool>,
}

impl ComposeState {
    pub fn new(
        choices: Vec<IdentityChoice>,
        chosen: usize,
        fields: DraftFields,
        overrides: (Option<bool>, Option<bool>),
    ) -> ComposeState {
        ComposeState {
            choices,
            chosen,
            fields: HeaderFields::from_draft(&fields),
            body: fields.body,
            in_reply_to: fields.in_reply_to,
            references: fields.references,
            sign_override: overrides.0,
            encrypt_override: overrides.1,
        }
    }

    pub fn identity(&self) -> &ComposeIdentity {
        &self.choices[self.chosen].identity
    }

    pub fn account(&self) -> &str {
        &self.choices[self.chosen].account
    }

    /// The seal plan as it stands: the chosen identity's
    /// default unless overridden by :sign/:nosign arming or
    /// the review screen's toggles.
    pub fn plan(&self) -> PgpPlan {
        PgpPlan {
            sign: self
                .sign_override
                .unwrap_or(self.identity().pgp_sign),
            encrypt: self.encrypt_override.unwrap_or(false),
        }
    }

    pub fn crypto(&self) -> ComposeCrypto {
        ComposeCrypto {
            plan: self.plan(),
            key: self.identity().pgp_key.clone(),
            address: self.identity().address.clone(),
        }
    }

    pub fn feed(&mut self, key: KeyEvent) -> HeadersOutcome {
        match self.fields.feed(key) {
            HeadersOutcome::CycleFrom(step) => {
                self.cycle_from(step);
                HeadersOutcome::Edited
            }
            other => other,
        }
    }

    fn cycle_from(&mut self, step: i32) {
        let count = self.choices.len() as i32;
        let next = (self.chosen as i32 + step).rem_euclid(count);
        self.chosen = next as usize;
    }

    pub fn sender_line(&self) -> String {
        let identity = self.identity();
        match &identity.name {
            Some(name) => format!("{name} <{}>", identity.address),
            None => identity.address.clone(),
        }
    }

    pub fn outgoing(&self) -> Result<Outgoing, String> {
        let identity = self.identity();
        if !identity.address.contains('@') {
            return Err("From needs a full address".to_string());
        }
        let to = address_list(&self.fields.to);
        if to.is_empty() {
            return Err("no recipients in To".to_string());
        }
        Ok(Outgoing {
            from_name: identity.name.clone(),
            from: identity.address.clone(),
            to,
            cc: address_list(&self.fields.cc),
            bcc: address_list(&self.fields.bcc),
            subject: self.fields.subject.trim().to_string(),
            in_reply_to: self.in_reply_to.clone(),
            references: self.references.clone(),
            body: self.body.clone(),
        })
    }
}

/// A compose validated and ready to assemble: parsed address
/// lists and the exact header values the message will carry.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Outgoing {
    pub from_name: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub body: String,
}

fn address_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(bare_address)
        .filter(|address| !address.is_empty())
        .collect()
}

pub(super) fn bare_address(value: &str) -> String {
    let bracketed = value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(inner, _)| inner);
    bracketed.unwrap_or(value).trim().to_string()
}

/// The one place an outgoing message is assembled; Bcc
/// recipients ride the envelope only, never the headers.
pub(super) fn assemble(outgoing: &Outgoing, date_unix: i64) -> Vec<u8> {
    let (_, domain) = outgoing
        .from
        .rsplit_once('@')
        .expect("validated in outgoing");
    let draft = Draft {
        from_name: outgoing.from_name.as_deref(),
        from: &outgoing.from,
        to: as_strs(&outgoing.to),
        cc: as_strs(&outgoing.cc),
        subject: &outgoing.subject,
        in_reply_to: outgoing.in_reply_to.as_deref(),
        references: as_strs(&outgoing.references),
        body: &outgoing.body,
        signature: None,
    };
    build_message(&draft, domain, date_unix)
}

pub(super) fn envelope(account: &str, outgoing: &Outgoing) -> Envelope {
    Envelope {
        account: account.to_string(),
        from: outgoing.from.clone(),
        recipients: outgoing
            .to
            .iter()
            .chain(&outgoing.cc)
            .chain(&outgoing.bcc)
            .cloned()
            .collect(),
    }
}

fn as_strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

#[cfg(test)]
pub(super) fn test_state() -> ComposeState {
    use super::testkit::tester_identity;

    let second = ComposeIdentity {
        name: None,
        address: "quin@example.org".to_string(),
        signature: None,
        pgp_sign: true,
        pgp_key: Some("ABCD".to_string()),
    };
    ComposeState::new(
        vec![
            IdentityChoice {
                account: "personal".to_string(),
                identity: tester_identity(),
            },
            IdentityChoice {
                account: "work".to_string(),
                identity: second,
            },
        ],
        0,
        DraftFields::default(),
        (None, None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_cycles_through_identities_and_wraps() {
        let mut state = test_state();
        assert_eq!(state.account(), "personal");
        assert_eq!(state.sender_line(), "Tester <tester@example.com>");
        state.cycle_from(1);
        assert_eq!(state.account(), "work");
        assert_eq!(state.sender_line(), "quin@example.org");
        state.cycle_from(1);
        assert_eq!(state.account(), "personal");
        state.cycle_from(-1);
        assert_eq!(state.account(), "work");
    }

    #[test]
    fn the_plan_follows_the_identity_until_overridden() {
        let mut state = test_state();
        assert_eq!(state.plan(), PgpPlan::default());
        state.cycle_from(1);
        assert!(state.plan().sign);
        assert_eq!(state.crypto().key.as_deref(), Some("ABCD"));
        state.sign_override = Some(false);
        state.encrypt_override = Some(true);
        assert!(!state.plan().sign);
        assert!(state.plan().encrypt);
        state.cycle_from(-1);
        assert!(state.plan().encrypt, "overrides survive cycling");
    }

    #[test]
    fn outgoing_requires_recipients_and_a_full_from() {
        let mut state = test_state();
        assert_eq!(
            state.outgoing().unwrap_err(),
            "no recipients in To"
        );
        state.fields.to = "alba@example.com".to_string();
        assert!(state.outgoing().is_ok());
        state.choices[0].identity.address = "not-an-address".into();
        assert_eq!(
            state.outgoing().unwrap_err(),
            "From needs a full address"
        );
    }

    #[test]
    fn recipient_fields_accept_commas_and_angle_brackets() {
        let mut state = test_state();
        state.fields.to =
            "Mara <mara@example.com>, quin@example.com".to_string();
        state.fields.cc = "<cc@example.com>".to_string();
        state.fields.bcc = "hidden@example.com".to_string();
        let outgoing = state.outgoing().unwrap();
        assert_eq!(
            outgoing.to,
            ["mara@example.com", "quin@example.com"]
        );
        assert_eq!(outgoing.cc, ["cc@example.com"]);
        let envelope = envelope("personal", &outgoing);
        assert_eq!(envelope.recipients.len(), 4);
        assert_eq!(envelope.from, "tester@example.com");
    }

    #[test]
    fn bcc_recipients_never_reach_the_headers() {
        let mut state = test_state();
        state.fields.to = "alba@example.com".to_string();
        state.fields.bcc = "hidden@example.com".to_string();
        state.fields.subject = "quiet".to_string();
        state.body = "Body here.\n".to_string();
        let outgoing = state.outgoing().unwrap();
        let raw = assemble(&outgoing, 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(!text.contains("hidden@example.com"), "{text}");
        assert!(
            envelope("personal", &outgoing)
                .recipients
                .contains(&"hidden@example.com".to_string())
        );
    }

    #[test]
    fn assembly_derives_the_message_id_domain_from_the_sender() {
        let mut state = test_state();
        state.fields.to = "alba@example.com".to_string();
        state.body = "Body here.\n".to_string();
        let outgoing = state.outgoing().unwrap();
        let raw = assemble(&outgoing, 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains(".antiphon@example.com>"));
        assert!(text.contains("format=flowed"));
        assert!(text.contains("Body here."));
    }

    #[test]
    fn threading_headers_ride_along_from_the_prefill() {
        let fields = DraftFields {
            to: "alba@example.com".to_string(),
            in_reply_to: Some("id-1@example.com".to_string()),
            references: vec!["id-1@example.com".to_string()],
            ..DraftFields::default()
        };
        let state = ComposeState::new(
            test_state().choices,
            0,
            fields,
            (None, None),
        );
        let outgoing = state.outgoing().unwrap();
        let raw = assemble(&outgoing, 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("In-Reply-To: <id-1@example.com>"));
        assert!(text.contains("References: <id-1@example.com>"));
    }
}
