use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_config::Composer;
use antiphon_store::{Outbox, StoreLayout};
use ratatui::DefaultTerminal;

use super::app::{App, View};
use super::compose;
use super::crypto;
use super::drafts;
use super::draw;
use super::editor::{EditorPane, EditorSession};

const DRAFTS_DIR: &str = "drafts";
const PASSED_TAG: &str = "passed";
const FALLBACK_EDITOR: &str = "vi";
const EDITOR_FAILED: &str =
    "editor exited with an error; body unchanged";

/// Hands the compose body to the editor: a body-only file,
/// never the headers, which stay structured fields. Embedded
/// runs the editor on a pty inside the client; suspend hands
/// the terminal over and settles on return.
pub(super) fn open_body_editor(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
) -> std::io::Result<()> {
    let Some(state) = &app.compose else {
        return Ok(());
    };
    if app.editor.is_some() {
        app.view = View::Editor;
        return Ok(());
    }
    let path = match write_body(layout, &state.body) {
        Ok(path) => path,
        Err(error) => {
            app.notice = Some(format!("draft: {error}"));
            return Ok(());
        }
    };
    let embedded = app.composer == Composer::Embedded
        && open_embedded(terminal, app, &path)?;
    if embedded {
        return Ok(());
    }
    let status = run_editor(terminal, &path);
    terminal.clear()?;
    match status {
        Ok(status) => finish_body_edit(app, &path, status.success()),
        Err(error) => app.notice = Some(format!("editor: {error}")),
    }
    Ok(())
}

fn open_embedded(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    path: &Path,
) -> std::io::Result<bool> {
    let size = terminal.size()?;
    let session = EditorSession::spawn(
        &editor_command(),
        path,
        draw::editor_rows(size.height),
        size.width,
    );
    let Ok(session) = session else {
        return Ok(false);
    };
    app.open_editor(EditorPane {
        path: path.to_path_buf(),
        session,
    });
    Ok(true)
}

/// The one place the terminal leaves ratatui's hands: restore,
/// run $EDITOR inheriting the tty, then take the screen back.
fn run_editor(
    terminal: &mut DefaultTerminal,
    path: &Path,
) -> std::io::Result<std::process::ExitStatus> {
    super::release_mouse();
    ratatui::restore();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$0\"", editor_command()))
        .arg(path)
        .status();
    *terminal = ratatui::init();
    super::grab_mouse();
    status
}

fn editor_command() -> String {
    std::env::var("EDITOR")
        .ok()
        .filter(|editor| !editor.trim().is_empty())
        .unwrap_or_else(|| FALLBACK_EDITOR.to_string())
}

fn write_body(
    layout: &StoreLayout,
    body: &str,
) -> std::io::Result<PathBuf> {
    let dir = layout.root().join(DRAFTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let name =
        format!("draft-{}-{}.eml", unix_now(), std::process::id());
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Settles a finished body edit onto the review screen: a
/// clean exit carries the new body along, a failed editor
/// keeps the previous one; either way nothing is discarded.
pub(super) fn finish_body_edit(
    app: &mut App,
    path: &Path,
    success: bool,
) {
    let Some(state) = app.compose.as_mut() else {
        return;
    };
    if success {
        match std::fs::read_to_string(path) {
            Ok(edited) => {
                // Quitting the first edit without writing
                // anything abandons the compose outright: an
                // untouched body means nothing worth keeping.
                let untouched = edited.trim() == state.body.trim();
                if untouched
                    && !state.reviewed
                    && state.attachments.is_empty()
                {
                    let _ = std::fs::remove_file(path);
                    app.compose = None;
                    app.view = View::List;
                    app.notice = Some("compose abandoned".to_string());
                    return;
                }
                state.body = edited;
            }
            Err(error) => {
                app.notice = Some(format!("draft: {error}"));
                return;
            }
        }
    } else {
        app.notice = Some(EDITOR_FAILED.to_string());
    }
    let _ = std::fs::remove_file(path);
    let Some(state) = app.compose.as_mut() else {
        return;
    };
    state.reviewed = true;
    app.view = View::Review;
}

/// Seals (signs and/or encrypts) the assembled message per
/// the compose plan and enqueues it. Any failure keeps the
/// review screen and the whole compose, so a message is never
/// sent unprotected or lost by accident.
pub(super) fn send_compose(app: &mut App, layout: &StoreLayout) {
    let Some(state) = &app.compose else {
        return;
    };
    let outgoing = match state.outgoing() {
        Ok(outgoing) => outgoing,
        Err(error) => {
            app.notice = Some(error);
            return;
        }
    };
    let raw =
        compose::assemble(&outgoing, &state.attachments, unix_now());
    let mut envelope = compose::envelope(state.account(), &outgoing);
    envelope.send_after = state.schedule;
    let sealed = match crypto::seal(
        &raw,
        &envelope.recipients,
        &state.crypto(),
        &app.keyring,
        None,
    ) {
        Ok(sealed) => sealed,
        Err(error) => {
            app.notice = Some(format!("not sent: {error}"));
            return;
        }
    };
    let forwarded = state.forwarded_of.clone();
    match Outbox::open(layout).enqueue(&envelope, &sealed) {
        Ok(_) => {
            if let Some((account, message_id)) = forwarded {
                app.pending_ops.push(super::actions::OpIntent::Flag {
                    account,
                    message_id,
                    add: vec![PASSED_TAG.to_string()],
                    remove: Vec::new(),
                });
            }
            app.discard_editor();
            app.compose = None;
            app.view = View::List;
            let subject = match outgoing.subject.is_empty() {
                true => String::new(),
                false => format!("{} ", outgoing.subject),
            };
            app.notice = Some(format!(
                "sending: {subject}to {} recipient(s)",
                envelope.recipients.len()
            ));
            super::nudge_daemon();
        }
        Err(error) => app.notice = Some(format!("outbox: {error}")),
    }
}

/// The review screen's q: the compose becomes a draft file
/// that :resume can reopen, fields and plan intact, and a
/// spooled message the daemon files in the account's server
/// drafts folder; the nudge makes that filing prompt.
pub(super) fn save_draft_and_close(
    app: &mut App,
    layout: &StoreLayout,
) {
    let Some(state) = &app.compose else {
        return;
    };
    match drafts::save(layout, state) {
        Ok(path) => {
            app.discard_editor();
            app.compose = None;
            app.view = View::List;
            app.notice =
                Some(format!("draft saved: {}", path.display()));
            super::nudge_daemon();
        }
        Err(error) => {
            app.notice = Some(format!("draft: {error}"));
        }
    }
}

pub(super) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
