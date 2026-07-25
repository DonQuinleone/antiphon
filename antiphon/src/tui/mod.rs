mod actions;
mod app;
mod attach;
mod commands;
mod complete;
mod compose;
mod crypto;
mod decrypt;
mod dispatch;
mod drafts;
mod draw;
mod drawer;
mod editor;
mod headers;
mod identity;
mod link_picker;
mod lists;
mod message_list;
mod pager;
mod pager_body;
mod patches;
mod prefill;
mod preview;
mod review;
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
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode,
    KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::crossterm::execute;

use antiphon_ipc::{
    IpcClient, OpId, OpKind, Operation, Request, socket_path,
};
use antiphon_pgp::Keyring;

use actions::{OpIntent, account_names};
use app::{App, DEFAULT_QUERY, KeyRoute, View};
use commands::PromptKind;
use dispatch::{
    dispatch, pending_resume_request, pending_template_request,
    pending_unsubscribe_request,
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
const MOUSE_WHEEL_ROWS: usize = 3;
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
    app.key_bindings = keymap
        .bindings()
        .iter()
        .map(|(action, text)| (text.clone(), action.to_string()))
        .collect();
    app.contacts = antiphon_store::contacts::load(layout);
    refresh_contacts(layout);
    app.apply(antiphon_core::Action::SidebarOpen);
    if app.take_requery() {
        let query = app.current_query.clone();
        run_query(&mut app, layout, query);
    }
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let outcome = event_loop(
        &mut terminal,
        &mut app,
        keymap,
        layout,
        &context,
        &loaded.config.saved_searches,
    );
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("terminal: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Re-harvests the contact ranking off the startup path; the
/// session uses the previous harvest, the next one gets this.
fn refresh_contacts(layout: &StoreLayout) {
    let layout = layout.clone();
    std::thread::spawn(move || {
        let Ok(index) = SearchIndex::open(&layout) else {
            return;
        };
        let _ = antiphon_store::contacts::harvest(&layout, &index);
    });
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
        tick_editor(terminal, app)?;
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
        let event = event::read()?;
        if let Event::Mouse(mouse) = event {
            pager_mouse(terminal, app, mouse)?;
            drain_ops(app);
            continue;
        }
        let Event::Key(key) = event else {
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
            KeyRoute::Review => review_key(terminal, app, layout, key)?,
            KeyRoute::Prompt => {
                prompt_key(app, layout, key);
                let mut request =
                    pending_template_request(app, context);
                if request.is_none() {
                    request = pending_unsubscribe_request(app, context);
                }
                if request.is_none() {
                    request = pending_resume_request(app, context);
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
    // ctrl-h lifts focus back to the header fields while the
    // editor keeps running underneath; ctrl-e returns.
    if key.code == KeyCode::Char('h')
        && key
            .modifiers
            .contains(ratatui::crossterm::event::KeyModifiers::CONTROL)
    {
        app.view = View::Compose;
        return;
    }
    if let Some(pane) = app.editor.as_mut() {
        pane.session.send_key(key);
    }
}

/// Esc in the fields stage backs out to wherever the compose
/// came from: the review screen once one exists, else out of
/// the compose entirely.
fn cancel_headers(app: &mut App) {
    let Some(state) = &app.compose else {
        return;
    };
    if state.reviewed {
        app.view = View::Review;
        return;
    }
    let has_content = !state.body.trim().is_empty()
        || !state.attachments.is_empty()
        || !state.fields.subject.trim().is_empty()
        || !state.fields.to.trim().is_empty();
    if has_content {
        app.open_prompt(PromptKind::ConfirmDraft);
        return;
    }
    app.abort_compose("compose aborted");
}

/// Keys on the review screen: toggles stay put, everything
/// else needs the terminal or the store and is acted on here.
fn review_key(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
    key: KeyEvent,
) -> std::io::Result<()> {
    use review::ReviewOutcome;

    let Some(state) = app.compose.as_mut() else {
        return Ok(());
    };
    match review::feed(state, key) {
        ReviewOutcome::Stay => {}
        ReviewOutcome::EditBody => {
            return session::open_body_editor(terminal, app, layout);
        }
        ReviewOutcome::EditHeaders => app.view = View::Compose,
        ReviewOutcome::PromptAttachment => {
            app.open_prompt(PromptKind::AttachmentPath)
        }
        ReviewOutcome::Send => session::send_compose(app, layout),
        ReviewOutcome::SaveDraft => {
            session::save_draft_and_close(app, layout)
        }
    }
    Ok(())
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
    if state.completion_key(key) {
        return Ok(());
    }
    match state.feed(key) {
        HeadersOutcome::Edited | HeadersOutcome::CycleFrom(_) => Ok(()),
        HeadersOutcome::OpenEditor => {
            session::open_body_editor(terminal, app, layout)
        }
        HeadersOutcome::Cancel => {
            cancel_headers(app);
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
    if app.help {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.help_scroll = app.help_scroll.saturating_add(1)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.help_scroll = app.help_scroll.saturating_sub(1)
            }
            _ => {
                app.help = false;
                app.help_scroll = 0;
            }
        }
        return;
    }
    if app.link_picker.is_some() {
        if let Some(url) = link_picker::feed(app, key) {
            link_picker::open_url(app, &url);
        }
        return;
    }
    if app.view == View::Pager && app.drawer_open {
        drawer::feed(app, key);
        return;
    }
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
    let count = if action.repeatable() {
        keymap.take_count()
    } else {
        1
    };
    for _ in 1..count {
        app.apply(action);
    }
    let request = dispatch(app, action, context);
    if app.take_requery() {
        let query = app.current_query.clone();
        run_query(app, layout, query);
    }
    if let Some(state) = request {
        app.start_compose(state);
    }
}

/// Mouse input serves the pager alone: the wheel scrolls it
/// and a left click on a link span hands the url to the
/// system opener, through the exact rows the draw produced.
fn pager_mouse(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mouse: MouseEvent,
) -> std::io::Result<()> {
    let receptive = app.view == View::Pager
        && app.prompt.is_none()
        && app.link_picker.is_none();
    if !receptive {
        return Ok(());
    }
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            wheel(app, antiphon_core::Action::MoveDown)
        }
        MouseEventKind::ScrollUp => {
            wheel(app, antiphon_core::Action::MoveUp)
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let size = terminal.size()?;
            click(app, size, mouse.column, mouse.row);
        }
        _ => {}
    }
    Ok(())
}

fn wheel(app: &mut App, action: antiphon_core::Action) {
    for _ in 0..MOUSE_WHEEL_ROWS {
        app.apply(action);
    }
}

fn click(
    app: &mut App,
    size: ratatui::layout::Size,
    column: u16,
    row: u16,
) {
    let area =
        ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let (content, _) = draw::split_status(area);
    let chrome = pager::chrome(app, content);
    let Some(url) =
        pager_body::link_url_at(app, chrome.body, column, row)
    else {
        return;
    };
    link_picker::open_url(app, &url);
}

/// Pump pty output into the parser, keep the pty sized to the
/// pane, and settle the compose once the child has exited.
fn tick_editor(
    terminal: &mut DefaultTerminal,
    app: &mut App,
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
    session::finish_body_edit(app, &pane.path, success);
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
    if app
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.kind == PromptKind::ConfirmDraft)
    {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                app.prompt = None;
                session::save_draft_and_close(app, layout);
            }
            KeyCode::Char('n' | 'N') => {
                app.prompt = None;
                app.abort_compose("compose discarded");
            }
            _ => {}
        }
        return;
    }
    if app.confirming_unsubscribe() {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        app.confirm_unsubscribe(confirmed);
        return;
    }
    let ctrl_c = key.code == KeyCode::Char('c')
        && key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl_c {
        app.prompt_cancel();
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
        PromptKind::AttachmentPath => {
            add_attachment(app, &prompt.buffer)
        }
        PromptKind::SaveAttachment => {
            drawer::save_selected(app, &prompt.buffer)
        }
        PromptKind::ConfirmUnsubscribe | PromptKind::ConfirmDraft => {}
    }
}

/// The review screen's a prompt settled: a readable file
/// joins the attachments, a bad path re-asks with the named
/// error alongside the answer to fix.
fn add_attachment(app: &mut App, input: &str) {
    match attach::load(input) {
        Ok(attachment) => {
            if let Some(state) = app.compose.as_mut() {
                state.add_attachment(attachment);
            }
        }
        Err(error) => {
            app.notice = Some(error);
            app.open_prompt(PromptKind::AttachmentPath);
            for ch in input.chars() {
                app.prompt_push(ch);
            }
        }
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
