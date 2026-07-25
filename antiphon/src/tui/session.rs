use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_config::Composer;
use antiphon_store::{Outbox, StoreLayout};
use ratatui::DefaultTerminal;

use super::app::App;
use super::compose::{self, ComposeState};
use super::crypto;
use super::draw;
use super::editor::{EditorPane, EditorSession};

const DRAFTS_DIR: &str = "drafts";
const FALLBACK_EDITOR: &str = "vi";
const COMPOSE_ABORTED: &str = "compose aborted";

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
        Ok(status) => {
            finish_body_edit(app, layout, &path, status.success())
        }
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
    ratatui::restore();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$0\"", editor_command()))
        .arg(path)
        .status();
    *terminal = ratatui::init();
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

/// Settles a finished body edit: a failed editor aborts the
/// compose, a clean exit queues the message with the current
/// fields and seal plan.
pub(super) fn finish_body_edit(
    app: &mut App,
    layout: &StoreLayout,
    path: &Path,
    success: bool,
) {
    if !success {
        let _ = std::fs::remove_file(path);
        app.abort_compose(COMPOSE_ABORTED);
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(edited) => {
            if let Some(state) = app.compose.as_mut() {
                state.body = edited;
            }
        }
        Err(error) => {
            app.notice = Some(format!("draft: {error}"));
            return;
        }
    }
    let Some(state) = app.compose.take() else {
        return;
    };
    app.view = super::app::View::List;
    let notice = queue_message(layout, app, &state, path);
    app.notice = Some(notice);
    super::nudge_daemon();
}

/// Seals (signs and/or encrypts) the assembled message per the
/// compose plan and enqueues it. Any pgp failure aborts the
/// send: the draft stays on disk and nothing reaches the
/// outbox, so a message is never sent unprotected by accident.
fn queue_message(
    layout: &StoreLayout,
    app: &App,
    state: &ComposeState,
    path: &Path,
) -> String {
    let outgoing = match state.outgoing() {
        Ok(outgoing) => outgoing,
        Err(error) => return error,
    };
    let raw = compose::assemble(&outgoing, unix_now());
    let envelope = compose::envelope(state.account(), &outgoing);
    let sealed = match crypto::seal(
        &raw,
        &envelope.recipients,
        &state.crypto(),
        &app.keyring,
        None,
    ) {
        Ok(sealed) => sealed,
        Err(error) => {
            return format!(
                "not sent: {error}; draft kept: {}",
                path.display()
            );
        }
    };
    match Outbox::open(layout).enqueue(&envelope, &sealed) {
        Ok(_) => {
            let _ = std::fs::remove_file(path);
            format!(
                "queued: {} to {} recipient(s)",
                outgoing.subject,
                envelope.recipients.len()
            )
        }
        Err(error) => format!("outbox: {error}"),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
