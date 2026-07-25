use std::path::{Path, PathBuf};

use antiphon_render::MessageHeader;

use super::app::App;

/// The reading pane shows the real body of the selected
/// message, loaded once per selection change. Encrypted mail
/// is not decrypted here: previewing must never wake
/// gpg-agent or pinentry while the user scrolls the list.
pub(super) const PREVIEW_LINE_CAP: usize = 400;
pub(super) const ENCRYPTED_NOTE: &str =
    "encrypted message \u{b7} open it to decrypt";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Preview {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub headers: Vec<MessageHeader>,
    pub headers_all: Vec<MessageHeader>,
}

pub(super) fn refresh(app: &mut App) {
    let Some(message) = app.selected_message() else {
        app.preview = None;
        return;
    };
    let path = message.path.clone();
    if app
        .preview
        .as_ref()
        .is_some_and(|preview| preview.path == path)
    {
        return;
    }
    let changed = app
        .preview
        .as_ref()
        .is_some_and(|preview| preview.path != path);
    if changed {
        app.preview_html = false;
    }
    let preview = load(&path, app.preview_html, &app.header_names);
    app.preview_scroll = 0;
    app.preview = Some(preview);
}

fn load(path: &Path, html: bool, names: &[String]) -> Preview {
    let Ok(raw) = std::fs::read(path) else {
        return Preview {
            path: path.to_owned(),
            lines: vec!["message file is unavailable".to_string()],
            headers: Vec::new(),
            headers_all: Vec::new(),
        };
    };
    Preview {
        path: path.to_owned(),
        lines: body_lines(&raw, html),
        headers: antiphon_render::selected_headers(&raw, names),
        headers_all: antiphon_render::all_headers(&raw),
    }
}

fn body_lines(raw: &[u8], html: bool) -> Vec<String> {
    if antiphon_pgp::encrypted_payload(raw).is_some() {
        return vec![ENCRYPTED_NOTE.to_string()];
    }
    let preference = if html {
        antiphon_render::BodyPreference::Html
    } else {
        antiphon_render::BodyPreference::Plain
    };
    antiphon_render::body_text_preferring(raw, preference)
        .text
        .lines()
        .take(PREVIEW_LINE_CAP)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &[u8] = b"From: a@example.com\n\
        To: b@example.com\n\
        Subject: preview\n\n\
        first line\nsecond line\n";

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn a_plain_message_previews_its_body_and_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.eml");
        std::fs::write(&path, PLAIN).unwrap();
        let preview = load(&path, false, &names(&["subject"]));
        assert_eq!(preview.lines[0], "first line");
        assert_eq!(preview.lines[1], "second line");
        assert_eq!(preview.headers.len(), 1);
        assert_eq!(preview.headers[0].name, "Subject");
        assert_eq!(preview.headers[0].value, "preview");
        assert_eq!(preview.headers_all.len(), 3);
    }

    #[test]
    fn a_missing_file_is_named_not_a_crash() {
        let preview = load(
            std::path::Path::new("/nowhere"),
            false,
            &names(&["from"]),
        );
        assert_eq!(preview.lines, ["message file is unavailable"]);
        assert!(preview.headers.is_empty());
    }
}
