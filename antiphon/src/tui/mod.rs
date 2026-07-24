mod app;
mod compose;
mod draw;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use antiphon_config::{Dirs, Loaded};
use antiphon_core::{Action, Keymap, Resolution};
use antiphon_store::{
    MessageSummary, Outbox, SearchError, SearchIndex, StoreLayout,
};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind,
};

use antiphon_ipc::{
    IpcClient, OpId, OpKind, Operation, Request, socket_path,
};

use app::{App, DEFAULT_QUERY, OpIntent, PromptKind, View, account_of};
use compose::{ComposeContext, ParsedDraft, ReplySource};

const INPUT_POLL: Duration = Duration::from_millis(250);
const REFRESH_EVERY: Duration = Duration::from_secs(2);
const LIST_WINDOW: usize = 500;
const DAEMON_ASSIGNS_ID: u64 = 0;
const UNREAD_QUERY: &str = "tag:unread";
const DRAFTS_DIR: &str = "drafts";
const FALLBACK_EDITOR: &str = "vi";
const COMPOSE_ABORTED: &str = "compose aborted";

pub fn run(
    loaded: &Loaded,
    layout: &StoreLayout,
    dirs: &Dirs,
) -> ExitCode {
    let keymap = match Keymap::new(&loaded.config.keys) {
        Ok(keymap) => keymap,
        Err(error) => {
            eprintln!("keymap: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (messages, total) = match query_window(layout, DEFAULT_QUERY) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("run `antiphon doctor` to check the setup");
            return ExitCode::FAILURE;
        }
    };
    let context = ComposeContext::from_loaded(loaded, dirs);
    let mut app = App::new(loaded, messages, total);
    let mut terminal = ratatui::init();
    let outcome =
        event_loop(&mut terminal, &mut app, keymap, layout, &context);
    ratatui::restore();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("terminal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn query_window(
    layout: &StoreLayout,
    query: &str,
) -> Result<(Vec<MessageSummary>, u32), SearchError> {
    let index = SearchIndex::open(layout)?;
    let messages = index.query(query, Some(LIST_WINDOW))?;
    let total = index.count(query)?;
    Ok((messages, total))
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut keymap: Keymap,
    layout: &StoreLayout,
    context: &ComposeContext,
) -> std::io::Result<()> {
    let mut last_refresh = Instant::now();
    let mut last_unread: Option<u32> = None;
    while !app.quit {
        let drawing = Instant::now();
        terminal.draw(|frame| draw::draw(frame, app))?;
        app.frame_stats.record(drawing.elapsed());
        if !event::poll(INPUT_POLL)? {
            drain_ops(app);
            maybe_refresh(
                app,
                layout,
                &mut last_refresh,
                &mut last_unread,
            );
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if app.prompt.is_some() {
            prompt_key(app, layout, key);
            continue;
        }
        if let Resolution::Match(action) = keymap.feed(key) {
            let request = dispatch(app, action, context);
            if let Some(request) = request {
                edit_and_queue(terminal, app, layout, request)?;
                nudge_daemon();
            }
        }
        drain_ops(app);
    }
    Ok(())
}

fn maybe_refresh(
    app: &mut App,
    layout: &StoreLayout,
    last_refresh: &mut Instant,
    last_unread: &mut Option<u32>,
) {
    if app.view != View::List || app.prompt.is_some() {
        return;
    }
    if last_refresh.elapsed() < REFRESH_EVERY {
        return;
    }
    *last_refresh = Instant::now();
    let query = app.current_query.clone();
    let Ok(index) = SearchIndex::open(layout) else {
        return;
    };
    let Ok(total) = index.count(&query) else {
        return;
    };
    let Ok(unread) = index.count(UNREAD_QUERY) else {
        return;
    };
    let unchanged =
        total == app.total_messages && *last_unread == Some(unread);
    *last_unread = Some(unread);
    if unchanged {
        return;
    }
    let Ok((messages, fresh_total)) = query_window(layout, &query)
    else {
        return;
    };
    let selected = app.selected;
    app.set_results(messages, fresh_total, query);
    app.selected = selected.min(app.messages.len().saturating_sub(1));
}

fn drain_ops(app: &mut App) {
    if app.pending_ops.is_empty() {
        return;
    }
    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        return;
    };
    while let Some(intent) = app.pending_ops.first().cloned() {
        let request = Request::EnqueueOp(wire_op(intent));
        let Ok(_) = client.request(&request) else {
            return;
        };
        app.pending_ops.remove(0);
    }
}

fn nudge_daemon() {
    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        return;
    };
    let _ = client.request(&Request::SyncNow);
}

fn wire_op(intent: OpIntent) -> Operation {
    let (account, message_id, kind) = match intent {
        OpIntent::Flag {
            account,
            message_id,
            add,
            remove,
        } => (account, message_id, OpKind::Flag { add, remove }),
        OpIntent::Delete {
            account,
            message_id,
        } => (account, message_id, OpKind::Delete),
    };
    Operation {
        op_id: OpId(DAEMON_ASSIGNS_ID),
        account,
        message_id,
        kind,
    }
}

fn prompt_key(app: &mut App, layout: &StoreLayout, key: KeyEvent) {
    match key.code {
        KeyCode::Char(ch) => app.prompt_push(ch),
        KeyCode::Backspace => app.prompt_backspace(),
        KeyCode::Esc => app.prompt_cancel(),
        KeyCode::Enter => submit_prompt(app, layout),
        _ => {}
    }
}

fn submit_prompt(app: &mut App, layout: &StoreLayout) {
    let Some(prompt) = app.prompt_submit() else {
        return;
    };
    match prompt.kind {
        PromptKind::Command => app.run_command(&prompt.buffer),
        PromptKind::Search => run_search(app, layout, prompt.buffer),
    }
}

fn run_search(app: &mut App, layout: &StoreLayout, raw: String) {
    let query = if raw.trim().is_empty() {
        DEFAULT_QUERY.to_string()
    } else {
        raw
    };
    match query_window(layout, &query) {
        Ok((messages, total)) => {
            app.set_results(messages, total, query)
        }
        Err(error) => app.notice = Some(error.to_string()),
    }
}

/// A draft ready for the user's editor; the event loop owns
/// the terminal hand-off, so app state never touches it.
struct EditorRequest {
    account: String,
    text: String,
}

fn dispatch(
    app: &mut App,
    action: Action,
    context: &ComposeContext,
) -> Option<EditorRequest> {
    if action == Action::Compose && app.view == View::List {
        app.notice = None;
        return fresh_request(app, context);
    }
    if action == Action::Reply {
        app.notice = None;
        return reply_request(app, context);
    }
    let opening = action == Action::Open && app.view == View::List;
    if !opening {
        app.apply(action);
        return None;
    }
    let message = app.selected_message()?;
    match std::fs::read(&message.path) {
        Ok(raw) => app.open_pager(body_text(&raw)),
        Err(error) => {
            app.open_pager(format!(
                "cannot read {}: {error}",
                message.path.display()
            ));
        }
    }
    None
}

fn body_text(raw: &[u8]) -> String {
    antiphon_render::body_text(raw).text
}

fn fresh_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
    let first = app.accounts.first().cloned().unwrap_or_default();
    let Some((account, identity)) = context.identity_for(&first) else {
        app.notice = Some("no compose identity configured".into());
        return None;
    };
    Some(EditorRequest {
        account: account.to_string(),
        text: compose::fresh_draft(identity),
    })
}

fn reply_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
    let Some(message) = app.selected_message().cloned() else {
        app.notice = Some("no message selected".into());
        return None;
    };
    let raw = match std::fs::read(&message.path) {
        Ok(raw) => raw,
        Err(error) => {
            app.notice = Some(format!(
                "cannot read {}: {error}",
                message.path.display()
            ));
            return None;
        }
    };
    let delivered = antiphon_render::delivered_addresses(&raw);
    let Some((account, identity)) = context
        .reply_identity_for(&account_of(&message.path), &delivered)
    else {
        app.notice = Some("no compose identity configured".into());
        return None;
    };
    let source = ReplySource {
        from: &message.from,
        subject: &message.subject,
        message_id: &message.id,
        date: &draw::format_date(
            message.date_unix,
            compose::ATTRIBUTION_DATE_FORMAT,
        ),
        body: &body_text(&raw),
    };
    Some(EditorRequest {
        account,
        text: compose::reply_draft(&identity, &source),
    })
}

fn edit_and_queue(
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
    let status = run_editor(terminal, &path);
    terminal.clear()?;
    app.notice = Some(match status {
        Ok(status) => finish_compose(layout, &request, &path, status),
        Err(error) => format!("editor: {error}"),
    });
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

fn finish_compose(
    layout: &StoreLayout,
    request: &EditorRequest,
    path: &Path,
    status: std::process::ExitStatus,
) -> String {
    if !status.success() {
        let _ = std::fs::remove_file(path);
        return COMPOSE_ABORTED.to_string();
    }
    let edited = match std::fs::read_to_string(path) {
        Ok(edited) => edited,
        Err(error) => return format!("draft: {error}"),
    };
    if compose::draft_unchanged(&request.text, &edited) {
        let _ = std::fs::remove_file(path);
        return COMPOSE_ABORTED.to_string();
    }
    match compose::parse_draft(&edited) {
        Ok(parsed) => {
            queue_message(layout, &request.account, &parsed, path)
        }
        Err(error) => error,
    }
}

fn queue_message(
    layout: &StoreLayout,
    account: &str,
    parsed: &ParsedDraft,
    path: &Path,
) -> String {
    let raw = compose::assemble(parsed, unix_now());
    let envelope = compose::envelope(account, parsed);
    match Outbox::open(layout).enqueue(&envelope, &raw) {
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
