mod actions;
mod app;
mod commands;
mod compose;
mod crypto;
mod decrypt;
mod dispatch;
mod draw;
mod editor;
mod headers;
mod identity;
mod lists;
mod message_list;
mod pager;
mod patches;
mod prefill;
mod preview;
mod scope;
mod session;
mod sidebar;
mod status;
#[cfg(test)]
mod testkit;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use antiphon_config::{Dirs, Loaded};
use antiphon_core::{Keymap, Resolution};
use antiphon_store::{
    MessageSummary, SearchError, SearchIndex, StoreLayout,
};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind,
};

use antiphon_ipc::{
    IpcClient, OpId, OpKind, Operation, Request, socket_path,
};
use antiphon_pgp::Keyring;

use actions::{OpIntent, account_names};
use app::{App, DEFAULT_QUERY, KeyRoute, View};
use commands::PromptKind;
use dispatch::{
    dispatch, pending_template_request, pending_unsubscribe_request,
};
use identity::ComposeContext;
use scope::ViewScope;

const INPUT_POLL: Duration = Duration::from_millis(250);
const EDITOR_POLL: Duration = Duration::from_millis(20);
/// A busy daemon (mid sync pass) answers nothing; the UI must
/// never hang on it. Queued work is durable either way.
const IPC_WAIT: Duration = Duration::from_secs(2);
const REFRESH_EVERY: Duration = Duration::from_secs(2);
const LIST_WINDOW: usize = 500;
const DAEMON_ASSIGNS_ID: u64 = 0;
const UNREAD_QUERY: &str = "tag:unread";
const PGP_KEYRING_DIR: &str = "pgp";

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
    let accounts = account_names(loaded);
    let effective = match scope::effective_query(
        &ViewScope::Unified,
        &accounts,
        DEFAULT_QUERY,
    ) {
        Ok(effective) => effective,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let (messages, total) = match query_window(layout, &effective) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("run `antiphon doctor` to check the setup");
            return ExitCode::FAILURE;
        }
    };
    let context = ComposeContext::from_loaded(loaded, dirs);
    let keyring = Keyring::from_dir(dirs.config.join(PGP_KEYRING_DIR));
    let folders = sidebar::discover(layout, &accounts);
    let mut app = App::new(loaded, &folders, messages, total, keyring);
    // Startup opens the default sidebar entry (the first
    // inbox), so the list shows it rather than highlighting
    // it over an unrelated query.
    app.apply(antiphon_core::Action::SidebarOpen);
    if app.take_requery() {
        let query = app.current_query.clone();
        run_query(&mut app, layout, query);
    }
    let mut terminal = ratatui::init();
    let outcome = event_loop(
        &mut terminal,
        &mut app,
        keymap,
        layout,
        &context,
        &loaded.config.saved_searches,
    );
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
    saved: &[antiphon_config::SavedSearch],
) -> std::io::Result<()> {
    let mut last_refresh = Instant::now();
    let mut last_unread: Option<u32> = None;
    while !app.quit {
        tick_editor(terminal, app, layout)?;
        preview::refresh(app);
        let drawing = Instant::now();
        terminal.draw(|frame| draw::draw(frame, app))?;
        app.frame_stats.record(drawing.elapsed());
        if !event::poll(poll_interval(app))? {
            drain_ops(app);
            maybe_refresh(
                app,
                layout,
                saved,
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
        match app.key_route() {
            KeyRoute::Editor => editor_key(app, key),
            KeyRoute::Compose => {
                compose_key(terminal, app, layout, key)?
            }
            KeyRoute::Prompt => {
                prompt_key(app, layout, key);
                let mut request =
                    pending_template_request(app, context);
                if request.is_none() {
                    request = pending_unsubscribe_request(app, context);
                }
                if let Some(state) = request {
                    app.start_compose(state);
                }
            }
            KeyRoute::Keymap => {
                keymap_key(app, &mut keymap, layout, context, key)
            }
        }
        drain_ops(app);
    }
    Ok(())
}

fn poll_interval(app: &App) -> Duration {
    if app.editor.is_some() {
        EDITOR_POLL
    } else {
        INPUT_POLL
    }
}

fn editor_key(app: &mut App, key: KeyEvent) {
    if let Some(pane) = app.editor.as_mut() {
        pane.session.send_key(key);
    }
}

/// Keys in the fields stage feed the header state machine;
/// the outcomes needing the terminal or store are acted on
/// here.
fn compose_key(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
    key: KeyEvent,
) -> std::io::Result<()> {
    use headers::HeadersOutcome;

    let Some(state) = app.compose.as_mut() else {
        return Ok(());
    };
    match state.feed(key) {
        HeadersOutcome::Edited | HeadersOutcome::CycleFrom(_) => Ok(()),
        HeadersOutcome::OpenEditor => {
            session::open_body_editor(terminal, app, layout)
        }
        HeadersOutcome::Cancel => {
            app.abort_compose("compose aborted");
            Ok(())
        }
    }
}

fn keymap_key(
    app: &mut App,
    keymap: &mut Keymap,
    layout: &StoreLayout,
    context: &ComposeContext,
    key: KeyEvent,
) {
    // Backspace scrolls the pager up out of the box (the
    // keymap holds one sequence per action, so this pairs
    // with enter's Open-as-scroll without stealing a
    // rebindable action; bind move-up = "backspace" to own
    // it globally).
    if app.view == View::Pager
        && app.prompt.is_none()
        && key.code == KeyCode::Backspace
    {
        app.apply(antiphon_core::Action::MoveUp);
        return;
    }
    let Resolution::Match(action) = keymap.feed(key) else {
        return;
    };
    let request = dispatch(app, action, context);
    if app.take_requery() {
        let query = app.current_query.clone();
        run_query(app, layout, query);
    }
    if let Some(state) = request {
        app.start_compose(state);
    }
}

/// Pump pty output into the parser, keep the pty sized to the
/// pane, and settle the compose once the child has exited.
fn tick_editor(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
) -> std::io::Result<()> {
    let Some(pane) = app.editor.as_mut() else {
        return Ok(());
    };
    pane.session.pump();
    let size = terminal.size()?;
    pane.session
        .resize(draw::editor_rows(size.height), size.width);
    let Some(success) = pane.session.exit_success() else {
        return Ok(());
    };
    let pane = app.close_editor().expect("editor pane present");
    session::finish_body_edit(app, layout, &pane.path, success);
    Ok(())
}

fn maybe_refresh(
    app: &mut App,
    layout: &StoreLayout,
    saved: &[antiphon_config::SavedSearch],
    last_refresh: &mut Instant,
    last_unread: &mut Option<u32>,
) {
    if last_refresh.elapsed() < REFRESH_EVERY {
        return;
    }
    *last_refresh = Instant::now();
    app.sync_progress = antiphon_sync::read_progress(layout);
    let folders = sidebar::discover(layout, &app.accounts);
    app.update_sidebar(sidebar::entries(&folders, saved));
    if app.view != View::List || app.prompt.is_some() {
        return;
    }
    let query = app.current_query.clone();
    let Ok(effective) = app.scoped(&query) else {
        return;
    };
    let Ok(unread_query) = app.scoped(UNREAD_QUERY) else {
        return;
    };
    let Ok(index) = SearchIndex::open(layout) else {
        return;
    };
    let Ok(total) = index.count(&effective) else {
        return;
    };
    let Ok(unread) = index.count(&unread_query) else {
        return;
    };
    let unchanged =
        total == app.total_messages && *last_unread == Some(unread);
    *last_unread = Some(unread);
    if unchanged {
        return;
    }
    let Ok((messages, fresh_total)) = query_window(layout, &effective)
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
    let _ = client.set_read_timeout(IPC_WAIT);
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
    let _ = client.set_read_timeout(IPC_WAIT);
    let _ = client.request(&Request::DrainOutbox);
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
    if app.confirming_unsubscribe() {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        app.confirm_unsubscribe(confirmed);
        return;
    }
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
        PromptKind::Command => {
            app.run_command(&prompt.buffer);
            patches::run_pending(app, layout);
        }
        PromptKind::Search => run_search(app, layout, prompt.buffer),
        PromptKind::ConfirmUnsubscribe => {}
    }
}

fn run_search(app: &mut App, layout: &StoreLayout, raw: String) {
    let query = if raw.trim().is_empty() {
        DEFAULT_QUERY.to_string()
    } else {
        raw
    };
    app.active_search = None;
    run_query(app, layout, query);
}

fn run_query(app: &mut App, layout: &StoreLayout, query: String) {
    let effective = match app.scoped(&query) {
        Ok(effective) => effective,
        Err(error) => {
            app.notice = Some(error.to_string());
            return;
        }
    };
    match query_window(layout, &effective) {
        Ok((messages, total)) => {
            app.set_results(messages, total, query)
        }
        Err(error) => app.notice = Some(error.to_string()),
    }
}
