use std::path::PathBuf;

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
    let lines = load_lines(&path);
    app.preview_scroll = 0;
    app.preview = Some(Preview { path, lines });
}

fn load_lines(path: &std::path::Path) -> Vec<String> {
    let Ok(raw) = std::fs::read(path) else {
        return vec!["message file is unavailable".to_string()];
    };
    if antiphon_pgp::encrypted_payload(&raw).is_some() {
        return vec![ENCRYPTED_NOTE.to_string()];
    }
    antiphon_render::body_text(&raw)
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

    #[test]
    fn a_plain_message_previews_its_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.eml");
        std::fs::write(&path, PLAIN).unwrap();
        let lines = load_lines(&path);
        assert_eq!(lines[0], "first line");
        assert_eq!(lines[1], "second line");
    }

    #[test]
    fn a_missing_file_is_named_not_a_crash() {
        let lines = load_lines(std::path::Path::new("/nowhere"));
        assert_eq!(lines, ["message file is unavailable"]);
    }
}
