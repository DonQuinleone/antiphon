use antiphon_store::StoreLayout;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

use antiphon_core::{Keymap, Resolution};

use super::app::{App, View};
use super::commands::PromptKind;
use super::dispatch::dispatch;
use super::identity::ComposeContext;
use super::settings::{self, SettingsOutcome};
use super::{
    account_form, attach, draw, drawer, folder_picker, headers,
    link_picker, mark_all_read, pager, pager_body, patches, review,
    run_query, run_search, session,
};

const MOUSE_WHEEL_ROWS: usize = 3;

pub(super) fn editor_key(app: &mut App, key: KeyEvent) {
    // ctrl-e (and ctrl-h) lift focus back to the header
    // fields while the editor keeps running underneath; the
    // same keys return, so one chord toggles.
    let control = key
        .modifiers
        .contains(ratatui::crossterm::event::KeyModifiers::CONTROL);
    if control && matches!(key.code, KeyCode::Char('e' | 'h')) {
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
pub(super) fn review_key(
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
            app.open_prompt(PromptKind::ConfirmDraft)
        }
    }
    Ok(())
}

/// Keys in the settings view: the account form, once open,
/// takes every key itself; everything else is a plain toggle
/// or selection settled in place.
pub(super) fn settings_key(app: &mut App, key: KeyEvent) {
    if app.account_form.is_some() {
        account_form::feed(app, key);
        return;
    }
    match settings::feed(app, key) {
        SettingsOutcome::Stay => {}
        SettingsOutcome::Close => {
            app.settings = None;
            app.view = View::List;
        }
    }
}

/// Keys in the fields stage feed the header state machine;
/// the outcomes needing the terminal or store are acted on
/// here.
pub(super) fn compose_key(
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

pub(super) fn keymap_key(
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
    if app.folder_picker.is_some() {
        folder_picker::feed(app, key);
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
    if action == antiphon_core::Action::MarkAllRead
        && app.view == View::List
    {
        mark_all_read::mark_all_read(app, layout);
        return;
    }
    if action == antiphon_core::Action::Settings {
        app.open_settings();
        return;
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
pub(super) fn pager_mouse(
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

pub(super) fn prompt_key(
    app: &mut App,
    layout: &StoreLayout,
    key: KeyEvent,
) {
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
            KeyCode::Esc => app.prompt = None,
            _ => {}
        }
        return;
    }
    if app.confirming_unsubscribe() {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        app.confirm_unsubscribe(confirmed);
        if let Some(url) = app.pending_unsub_post.take() {
            app.notice = Some(post_one_click(url));
        }
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

/// A confirmed RFC 8058 unsubscribe goes to antiphond over
/// IPC and the POST happens there, so the client stays off
/// the network.
fn post_one_click(url: String) -> String {
    use antiphon_ipc::{IpcClient, Request, Response, socket_path};

    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        return "unsubscribe: antiphond is not running".to_string();
    };
    let _ = client.set_read_timeout(super::IPC_WAIT);
    match client.request(&Request::Unsubscribe { url }) {
        Ok(Response::Ack) => {
            "unsubscribing: POST handed to antiphond".to_string()
        }
        Ok(Response::Error(error)) => format!("unsubscribe: {error}"),
        Ok(_) => "unsubscribe: unexpected daemon reply".to_string(),
        Err(error) => format!("unsubscribe: {error}"),
    }
}
