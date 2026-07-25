use std::io;
use std::path::{Path, PathBuf};

use antiphon_store::{DraftEnvelope, DraftSpool, StoreLayout};

use super::compose::{ComposeState, bare_address};
use super::prefill::DraftFields;

const DRAFTS_DIR: &str = "drafts";
const ON: &str = "on";
const OFF: &str = "off";

/// A compose saved from the review screen, restored by
/// :resume with its fields, plan and attachment paths
/// intact.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SavedDraft {
    pub from_name: Option<String>,
    pub from: String,
    pub account: String,
    pub fields: DraftFields,
    pub sign: Option<bool>,
    pub encrypt: Option<bool>,
    pub attachments: Vec<PathBuf>,
}

pub(super) fn save(
    layout: &StoreLayout,
    state: &ComposeState,
) -> io::Result<PathBuf> {
    let dir = layout.root().join(DRAFTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "draft-{}-{}.eml",
        super::session::unix_now(),
        std::process::id()
    );
    let path = dir.join(name);
    std::fs::write(&path, render(state))?;
    spool(layout, state)?;
    Ok(path)
}

/// The unsealed RFC 5322 message, attachments included, goes
/// to the daemon's draft spool for filing in the account's
/// server drafts folder; the private file written above stays
/// what :resume reads back. A From without a full address
/// cannot become a message yet, so only the private file is
/// kept.
fn spool(layout: &StoreLayout, state: &ComposeState) -> io::Result<()> {
    let outgoing = state.draft_outgoing();
    if !outgoing.from.contains('@') {
        return Ok(());
    }
    let raw = super::compose::assemble(
        &outgoing,
        &state.attachments,
        super::session::unix_now(),
    );
    let envelope = DraftEnvelope {
        account: state.account().to_string(),
    };
    DraftSpool::open(layout)
        .enqueue(&envelope, &raw)
        .map_err(io::Error::other)?;
    Ok(())
}

fn render(state: &ComposeState) -> String {
    let plan = state.plan();
    let mut out = format!(
        "From: {}\nAccount: {}\nTo: {}\nCc: {}\nBcc: {}\n\
         Subject: {}\n",
        state.sender_line(),
        state.account(),
        state.fields.to,
        state.fields.cc,
        state.fields.bcc,
        state.fields.subject,
    );
    if let Some(id) = &state.in_reply_to {
        out.push_str(&format!("In-Reply-To: <{id}>\n"));
    }
    if !state.references.is_empty() {
        let ids: Vec<String> = state
            .references
            .iter()
            .map(|id| format!("<{id}>"))
            .collect();
        out.push_str(&format!("References: {}\n", ids.join(" ")));
    }
    out.push_str(&format!(
        "Sign: {}\nEncrypt: {}\n",
        toggle(plan.sign),
        toggle(plan.encrypt)
    ));
    for attachment in &state.attachments {
        out.push_str(&format!(
            "Attachment: {}\n",
            attachment.path.display()
        ));
    }
    out.push('\n');
    out.push_str(&state.body);
    out
}

fn toggle(value: bool) -> &'static str {
    if value { ON } else { OFF }
}

pub(super) fn load(path: &Path) -> Result<SavedDraft, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    parse(&text)
}

fn parse(text: &str) -> Result<SavedDraft, String> {
    let Some((headers, body)) = text.split_once("\n\n") else {
        return Err(
            "draft has no blank line after the headers".to_string()
        );
    };
    let mut draft = SavedDraft {
        fields: DraftFields {
            body: body.to_string(),
            ..DraftFields::default()
        },
        ..SavedDraft::default()
    };
    for line in headers.lines() {
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("malformed draft line: {line}"));
        };
        apply(&mut draft, key.trim(), value.trim())?;
    }
    if !draft.from.contains('@') {
        return Err("draft has no From address".to_string());
    }
    Ok(draft)
}

fn apply(
    draft: &mut SavedDraft,
    key: &str,
    value: &str,
) -> Result<(), String> {
    match key.to_ascii_lowercase().as_str() {
        "from" => set_from(draft, value),
        "account" => draft.account = value.to_string(),
        "to" => draft.fields.to = value.to_string(),
        "cc" => draft.fields.cc = value.to_string(),
        "bcc" => draft.fields.bcc = value.to_string(),
        "subject" => draft.fields.subject = value.to_string(),
        "in-reply-to" => draft.fields.in_reply_to = single_id(value),
        "references" => draft.fields.references = id_list(value),
        "sign" => draft.sign = Some(value == ON),
        "encrypt" => draft.encrypt = Some(value == ON),
        "attachment" => draft.attachments.push(value.into()),
        other => {
            return Err(format!("unknown draft header: {other}"));
        }
    }
    Ok(())
}

fn set_from(draft: &mut SavedDraft, value: &str) {
    draft.from = bare_address(value);
    let name = value
        .split_once('<')
        .map(|(name, _)| name.trim().trim_matches('"').trim())
        .unwrap_or("");
    draft.from_name = (!name.is_empty()).then(|| name.to_string());
}

fn single_id(value: &str) -> Option<String> {
    let id = value.trim().trim_matches(['<', '>']).to_string();
    (!id.is_empty()).then_some(id)
}

fn id_list(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(|id| id.trim_matches(['<', '>']).to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::compose::test_state;
    use super::*;

    use super::super::attach;
    use super::super::testkit::TempDir;

    fn saved_state() -> ComposeState {
        let mut state = test_state();
        state.fields.to = "alba@example.com, bo@example.com".into();
        state.fields.bcc = "hidden@example.com".into();
        state.fields.subject = "Rehearsal".into();
        state.in_reply_to = Some("id-1@example.com".into());
        state.references = vec!["id-0@example.com".into()];
        state.encrypt_override = Some(true);
        state.body = "Body line.\n\nSecond.\n".into();
        state
    }

    #[test]
    fn drafts_round_trip_fields_plan_and_body() {
        let state = saved_state();
        let draft = parse(&render(&state)).unwrap();
        assert_eq!(draft.from, "tester@example.com");
        assert_eq!(draft.from_name.as_deref(), Some("Tester"));
        assert_eq!(draft.account, "personal");
        assert_eq!(draft.fields.to, "alba@example.com, bo@example.com");
        assert_eq!(draft.fields.bcc, "hidden@example.com");
        assert_eq!(draft.fields.subject, "Rehearsal");
        assert_eq!(
            draft.fields.in_reply_to.as_deref(),
            Some("id-1@example.com")
        );
        assert_eq!(draft.fields.references, ["id-0@example.com"]);
        assert_eq!(draft.sign, Some(false));
        assert_eq!(draft.encrypt, Some(true));
        assert_eq!(draft.fields.body, "Body line.\n\nSecond.\n");
    }

    #[test]
    fn attachments_survive_save_and_restore_by_path() {
        let dir = TempDir::new();
        let path = dir.path.join("notes.txt");
        std::fs::write(&path, b"kept bytes").unwrap();
        let mut state = saved_state();
        state.add_attachment(
            attach::load(path.to_str().unwrap()).unwrap(),
        );
        let draft = parse(&render(&state)).unwrap();
        assert_eq!(draft.attachments, std::slice::from_ref(&path));
        let restored =
            attach::load(draft.attachments[0].to_str().unwrap())
                .unwrap();
        assert_eq!(restored.bytes, b"kept bytes");
        assert_eq!(restored.content_type, "text/plain");
    }

    #[test]
    fn broken_drafts_fail_naming_the_problem() {
        assert_eq!(
            parse("no headers").unwrap_err(),
            "draft has no blank line after the headers"
        );
        assert!(
            parse("From: a@b.c\nnonsense\n\nhi")
                .unwrap_err()
                .contains("malformed draft line")
        );
        assert_eq!(
            parse("From: a@b.c\nX-Odd: yes\n\nhi").unwrap_err(),
            "unknown draft header: x-odd"
        );
        assert_eq!(
            parse("To: a@b.c\n\nhi").unwrap_err(),
            "draft has no From address"
        );
    }

    #[test]
    fn saving_spools_the_message_and_keeps_resume_state() {
        use mail_parser::{MessageParser, MimeHeaders};

        let dir = TempDir::new();
        let layout = StoreLayout::new(dir.path.join("store"));
        let attachment_path = dir.path.join("notes.txt");
        std::fs::write(&attachment_path, b"kept bytes").unwrap();
        let mut state = saved_state();
        state.add_attachment(
            attach::load(attachment_path.to_str().unwrap()).unwrap(),
        );

        let path = save(&layout, &state).unwrap();
        let resumed = load(&path).unwrap();
        assert_eq!(resumed.fields.subject, "Rehearsal");
        assert_eq!(
            resumed.attachments,
            std::slice::from_ref(&attachment_path)
        );

        let spooled = DraftSpool::open(&layout).pending().unwrap();
        assert_eq!(spooled.len(), 1);
        assert_eq!(spooled[0].account, "personal");
        let raw = std::fs::read(&spooled[0].message_path).unwrap();
        let message = MessageParser::default().parse(&raw).unwrap();
        assert_eq!(message.subject(), Some("Rehearsal"));
        let part = message
            .attachments()
            .find(|part| part.attachment_name() == Some("notes.txt"))
            .expect("the attachment inside the spooled draft");
        assert_eq!(part.contents(), b"kept bytes");
        let text = String::from_utf8_lossy(&raw);
        assert!(
            !text.contains("hidden@example.com"),
            "bcc leaked into the spooled draft: {text}"
        );
    }

    #[test]
    fn an_unaddressed_from_saves_locally_without_spooling() {
        let dir = TempDir::new();
        let layout = StoreLayout::new(dir.path.join("store"));
        let mut state = saved_state();
        state.choices[0].identity.address = "not-an-address".into();
        let path = save(&layout, &state).unwrap();
        assert!(path.is_file());
        assert!(
            DraftSpool::open(&layout).pending().unwrap().is_empty()
        );
    }

    #[test]
    fn loading_a_missing_draft_names_the_path() {
        let error =
            load(Path::new("/nonexistent/draft.eml")).unwrap_err();
        assert!(error.starts_with("/nonexistent/draft.eml:"));
    }
}
