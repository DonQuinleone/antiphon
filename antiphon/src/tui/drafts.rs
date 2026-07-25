use std::path::{Path, PathBuf};

use antiphon_store::StoreLayout;

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
) -> std::io::Result<PathBuf> {
    let dir = layout.root().join(DRAFTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "draft-{}-{}.eml",
        super::session::unix_now(),
        std::process::id()
    );
    let path = dir.join(name);
    std::fs::write(&path, render(state))?;
    Ok(path)
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
    fn loading_a_missing_draft_names_the_path() {
        let error =
            load(Path::new("/nonexistent/draft.eml")).unwrap_err();
        assert!(error.starts_with("/nonexistent/draft.eml:"));
    }
}
