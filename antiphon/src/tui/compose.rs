use antiphon_store::contacts::Contact;
use ratatui::crossterm::event::KeyEvent;

use super::attach::Attachment;
use super::complete::Completion;
use super::compose_assembly::address_list;
pub(super) use super::compose_assembly::{
    Outgoing, assemble, bare_address, envelope,
};
use super::crypto::{ComposeCrypto, PgpPlan};
use super::headers::HeaderFields;
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
    /// Whether the outgoing message asks for a read receipt;
    /// toggled on the review screen, off by default.
    pub read_receipt: bool,
    pub attachments: Vec<Attachment>,
    pub selected_attachment: usize,
    pub reviewed: bool,
    pub contacts: Vec<Contact>,
    pub completion: Option<Completion>,
    pub forwarded_of: Option<(String, String)>,
    /// Unix time at or after which the message may send; None
    /// sends at the next outbox drain. Set on the review screen.
    pub schedule: Option<u64>,
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
            read_receipt: false,
            attachments: Vec::new(),
            selected_attachment: 0,
            reviewed: false,
            contacts: Vec::new(),
            completion: None,
            forwarded_of: None,
            schedule: None,
        }
    }

    pub fn add_attachment(&mut self, attachment: Attachment) {
        self.attachments.push(attachment);
        self.selected_attachment = self.attachments.len() - 1;
    }

    pub fn remove_selected_attachment(&mut self) {
        if self.selected_attachment >= self.attachments.len() {
            return;
        }
        self.attachments.remove(self.selected_attachment);
        self.selected_attachment = self
            .selected_attachment
            .min(self.attachments.len().saturating_sub(1));
    }

    pub fn select_attachment(&mut self, step: i32) {
        let count = self.attachments.len() as i32;
        if count == 0 {
            return;
        }
        let next =
            (self.selected_attachment as i32 + step).rem_euclid(count);
        self.selected_attachment = next as usize;
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

    /// A literal keystroke for the focused field: text editing,
    /// or an identity cycle when From holds focus. Focus, submit
    /// and cancel are compose actions settled before this.
    pub fn edit(&mut self, key: KeyEvent) {
        if let Some(step) = self.fields.edit(key) {
            self.cycle_from(step);
        }
        self.refresh_completion();
    }

    pub fn step_focus(&mut self, step: i32) {
        self.fields.step_focus(step);
        self.refresh_completion();
    }

    pub fn at_last_field(&self) -> bool {
        self.fields.at_last_field()
    }

    pub fn close_completion(&mut self) {
        self.completion = None;
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
        let outgoing = self.draft_outgoing();
        if !outgoing.from.contains('@') {
            return Err("From needs a full address".to_string());
        }
        if outgoing.to.is_empty() {
            return Err("no recipients in To".to_string());
        }
        Ok(outgoing)
    }

    /// The compose exactly as it stands, unvalidated: a draft
    /// may lack recipients and still deserves saving.
    pub fn draft_outgoing(&self) -> Outgoing {
        let identity = self.identity();
        Outgoing {
            from_name: identity.name.clone(),
            from: identity.address.clone(),
            to: address_list(&self.fields.to),
            cc: address_list(&self.fields.cc),
            bcc: address_list(&self.fields.bcc),
            subject: self.fields.subject.trim().to_string(),
            in_reply_to: self.in_reply_to.clone(),
            references: self.references.clone(),
            body: self.body.clone(),
            read_receipt: self.read_receipt,
        }
    }
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
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

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
            ["Mara <mara@example.com>", "quin@example.com"],
            "headers keep the display name as typed"
        );
        assert_eq!(outgoing.cc, ["cc@example.com"]);
        let envelope = envelope("personal", &outgoing);
        assert_eq!(envelope.recipients.len(), 4);
        assert!(
            envelope
                .recipients
                .contains(&"mara@example.com".to_string()),
            "the envelope reduces entries to bare addresses"
        );
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
        let raw = assemble(&outgoing, &[], 1_753_380_000);
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
        let raw = assemble(&outgoing, &[], 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains(".antiphon@example.com>"));
        assert!(text.contains("format=flowed"));
        assert!(text.contains("Body here."));
    }

    #[test]
    fn attachment_selection_clamps_wraps_and_removes() {
        use super::super::attach::Attachment;

        fn file(name: &str) -> Attachment {
            Attachment {
                path: name.into(),
                filename: name.to_string(),
                content_type: "text/plain",
                bytes: Vec::new(),
            }
        }

        let mut state = test_state();
        state.select_attachment(1);
        state.remove_selected_attachment();
        assert!(state.attachments.is_empty(), "empty list is safe");

        state.add_attachment(file("a.txt"));
        state.add_attachment(file("b.txt"));
        state.add_attachment(file("c.txt"));
        assert_eq!(state.selected_attachment, 2, "newest selected");
        state.select_attachment(1);
        assert_eq!(state.selected_attachment, 0, "wraps forward");
        state.select_attachment(-1);
        assert_eq!(state.selected_attachment, 2, "wraps back");
        state.remove_selected_attachment();
        assert_eq!(state.attachments.len(), 2);
        assert_eq!(state.selected_attachment, 1, "clamped to last");
        state.select_attachment(-1);
        state.remove_selected_attachment();
        assert_eq!(state.attachments[0].filename, "b.txt");
    }

    #[test]
    fn typing_a_recipient_offers_and_accepts_completion() {
        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent::new(code, KeyModifiers::NONE)
        }

        let mut state = test_state();
        state.contacts = vec![Contact {
            address: "alba@example.com".to_string(),
            name: "Alba Voss".to_string(),
            score: 7,
        }];
        assert!(!state.completion_key(key(KeyCode::Tab)));
        for ch in "al".chars() {
            state.edit(key(KeyCode::Char(ch)));
        }
        assert!(state.completion.is_some());
        assert!(state.completion_key(key(KeyCode::Tab)));
        assert_eq!(state.fields.to, "Alba Voss <alba@example.com>");
        assert!(state.completion.is_none());

        for ch in ", al".chars() {
            state.edit(key(KeyCode::Char(ch)));
        }
        assert!(state.completion_key(key(KeyCode::Esc)));
        assert!(state.completion.is_none(), "esc dismisses only");
        state.edit(key(KeyCode::Char('b')));
        assert!(state.completion.is_some(), "typing re-offers");
        assert!(state.completion_key(key(KeyCode::Esc)));
        state.step_focus(1);
        assert_eq!(state.fields.focus, 1, "tab moves focus again");
        assert!(state.completion.is_none());
    }

    #[test]
    fn a_read_receipt_flag_reaches_outgoing_and_the_header() {
        let mut state = test_state();
        state.fields.to = "alba@example.com".to_string();
        assert!(!state.draft_outgoing().read_receipt, "off default");
        state.read_receipt = true;
        let outgoing = state.outgoing().unwrap();
        assert!(outgoing.read_receipt);
        let raw = assemble(&outgoing, &[], 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains(
            "Disposition-Notification-To: <tester@example.com>"
        ));
    }

    #[test]
    fn the_review_toggle_flips_the_read_receipt_field() {
        use super::super::app::View;
        use super::super::testkit::app_with_messages;

        let mut app = app_with_messages(1);
        app.compose = Some(test_state());
        app.view = View::Review;
        assert!(!app.compose.as_ref().unwrap().read_receipt);
        app.apply(antiphon_core::Action::ToggleReadReceipt);
        assert!(app.compose.as_ref().unwrap().read_receipt);
        app.apply(antiphon_core::Action::ToggleReadReceipt);
        assert!(!app.compose.as_ref().unwrap().read_receipt);
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
        let raw = assemble(&outgoing, &[], 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("In-Reply-To: <id-1@example.com>"));
        assert!(text.contains("References: <id-1@example.com>"));
    }
}
