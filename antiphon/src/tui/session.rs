use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_config::Composer;
use antiphon_pgp::Keyring;
use antiphon_store::{Outbox, StoreLayout};
use ratatui::DefaultTerminal;

use super::app::App;
use super::compose::{self, ParsedDraft};
use super::crypto::{self, ComposeCrypto};
use super::dispatch::EditorRequest;
use super::draw;
use super::editor::{EditorPane, EditorSession};

const DRAFTS_DIR: &str = "drafts";
const FALLBACK_EDITOR: &str = "vi";
const COMPOSE_ABORTED: &str = "compose aborted";

pub(super) fn begin_compose(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
    request: EditorRequest,
) -> std::io::Result<()> {
    let path = match write_draft(layout, &request.text) {
        Ok(path) => path,
        Err(error) => {
            app.notice = Some(format!("draft: {error}"));
            return Ok(());
        }
    };
    let embedded = app.composer == Composer::Embedded
        && open_embedded(terminal, app, &request, &path)?;
    if embedded {
        return Ok(());
    }
    suspend_compose(terminal, app, layout, &request, &path)
}

/// The embedded default: the editor child runs on a pty and
/// its screen renders inside the client. Returns false when
/// the pty cannot be created, handing over to the suspend
/// fallback.
fn open_embedded(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    request: &EditorRequest,
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
        account: request.account.clone(),
        written: request.text.clone(),
        path: path.to_path_buf(),
        crypto: request.crypto.clone(),
        session,
    });
    Ok(true)
}

fn suspend_compose(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
    request: &EditorRequest,
    path: &Path,
) -> std::io::Result<()> {
    let status = run_editor(terminal, path);
    terminal.clear()?;
    app.notice = Some(match status {
        Ok(status) => finish_compose(
            layout,
            &app.keyring,
            &request.account,
            &request.text,
            path,
            &request.crypto,
            status.success(),
        ),
        Err(error) => format!("editor: {error}"),
    });
    super::nudge_daemon();
    Ok(())
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

fn write_draft(
    layout: &StoreLayout,
    text: &str,
) -> std::io::Result<PathBuf> {
    let dir = layout.root().join(DRAFTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let name =
        format!("draft-{}-{}.eml", unix_now(), std::process::id());
    let path = dir.join(name);
    std::fs::write(&path, text)?;
    Ok(path)
}

pub(super) fn finish_compose(
    layout: &StoreLayout,
    keyring: &Keyring,
    account: &str,
    written: &str,
    path: &Path,
    crypto: &ComposeCrypto,
    success: bool,
) -> String {
    if !success {
        let _ = std::fs::remove_file(path);
        return COMPOSE_ABORTED.to_string();
    }
    let edited = match std::fs::read_to_string(path) {
        Ok(edited) => edited,
        Err(error) => return format!("draft: {error}"),
    };
    if compose::draft_unchanged(written, &edited) {
        let _ = std::fs::remove_file(path);
        return COMPOSE_ABORTED.to_string();
    }
    match compose::parse_draft(&edited) {
        Ok(parsed) => queue_message(
            layout, keyring, account, &parsed, path, crypto,
        ),
        Err(error) => error,
    }
}

/// Seals (signs and/or encrypts) the assembled message per the
/// compose plan and enqueues it. Any pgp failure aborts the
/// send: the draft stays on disk and nothing reaches the
/// outbox, so a message is never sent unprotected by accident.
fn queue_message(
    layout: &StoreLayout,
    keyring: &Keyring,
    account: &str,
    parsed: &ParsedDraft,
    path: &Path,
    crypto: &ComposeCrypto,
) -> String {
    let raw = compose::assemble(parsed, unix_now());
    let envelope = compose::envelope(account, parsed);
    let sealed = match crypto::seal(
        &raw,
        &envelope.recipients,
        crypto,
        keyring,
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
                parsed.subject,
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
