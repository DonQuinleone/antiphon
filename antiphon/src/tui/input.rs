use antiphon_store::StoreLayout;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

use antiphon_core::{Action, Context, Keymap, Resolution};

use super::app::{App, View};
use super::commands::PromptKind;
use super::dispatch::dispatch;
use super::identity::ComposeContext;
use super::{
    account_form, attach, bulk, draw, drawer, export, folder_alias,
    folder_picker, link_picker, mark_all_read, pager, pager_body,
    patches, run_query, run_search, session,
};
use crate::tui::settings::{self, SettingsTab};

const MOUSE_WHEEL_ROWS: usize = 3;

const READ_ONLY_NOTICE: &str = "read-only archive view";
const MUTATING_ACTIONS: [Action; 12] = [
    Action::Compose,
    Action::Reply,
    Action::ReplyAll,
    Action::ReplyList,
    Action::Forward,
    Action::ToggleRead,
    Action::ToggleFlagged,
    Action::MarkAllRead,
    Action::DeleteMessage,
    Action::Archive,
    Action::MoveTo,
    Action::Sync,
];

fn read_only_blocks(app: &mut App, action: Action) -> bool {
    if !app.read_only || !MUTATING_ACTIONS.contains(&action) {
        return false;
    }
    app.notice = Some(READ_ONLY_NOTICE.to_string());
    true
}

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
    keymap: &mut Keymap,
    layout: &StoreLayout,
    key: KeyEvent,
) -> std::io::Result<()> {
    if app.schedule_edit.is_some() {
        super::schedule::feed_edit(app, keymap, key);
        return Ok(());
    }
    if app.compose.is_none() {
        return Ok(());
    }
    let Resolution::Match(action) = keymap.feed(Context::Review, key)
    else {
        return Ok(());
    };
    match action {
        Action::Send => session::send_compose(app, layout),
        Action::EditBody => {
            return session::open_body_editor(terminal, app, layout);
        }
        Action::EditHeaders => app.view = View::Compose,
        Action::AttachFile => {
            app.open_prompt(PromptKind::AttachmentPath)
        }
        Action::SaveDraft => app.open_prompt(PromptKind::ConfirmDraft),
        Action::Schedule => super::schedule::begin(app),
        _ => app.apply(action),
    }
    Ok(())
}

/// Keys in the settings view: the account form, once open,
/// takes every key itself; everything else is a plain toggle
/// or selection settled in place.
pub(super) fn settings_key(
    app: &mut App,
    keymap: &mut Keymap,
    key: KeyEvent,
) {
    if app.account_form.is_some() {
        account_form::feed(app, key);
        return;
    }
    if app.folder_alias_edit.is_some() {
        folder_alias::feed_edit(app, keymap, key);
        return;
    }
    if settings::feed_modal(app, key) {
        return;
    }
    let context = context_for(app);
    if let Resolution::Match(action) = keymap.feed(context, key) {
        settings::dispatch(app, action);
    }
}

/// Keys in the fields stage feed the header state machine;
/// the outcomes needing the terminal or store are acted on
/// here.
pub(super) fn compose_key(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    keymap: &mut Keymap,
    layout: &StoreLayout,
    key: KeyEvent,
) -> std::io::Result<()> {
    if app.compose.is_none() {
        return Ok(());
    }
    if let Some(state) = app.compose.as_mut()
        && state.completion_key(key)
    {
        return Ok(());
    }
    match keymap.feed(Context::Compose, key) {
        Resolution::Match(Action::FocusNext) => {
            step_compose(app, 1);
            Ok(())
        }
        Resolution::Match(Action::FocusPrev) => {
            step_compose(app, -1);
            Ok(())
        }
        Resolution::Match(Action::ComposeSubmit) => {
            submit_compose(terminal, app, layout)
        }
        Resolution::Match(Action::OpenEditor) => {
            open_compose_editor(terminal, app, layout)
        }
        Resolution::Match(Action::ComposeCancel) => {
            if let Some(state) = app.compose.as_mut() {
                state.close_completion();
            }
            cancel_headers(app);
            Ok(())
        }
        _ => {
            if let Some(state) = app.compose.as_mut() {
                state.edit(key);
            }
            Ok(())
        }
    }
}

fn step_compose(app: &mut App, step: i32) {
    if let Some(state) = app.compose.as_mut() {
        state.step_focus(step);
    }
}

/// Enter in the fields: the last field opens the body editor,
/// any earlier one steps to the next.
fn submit_compose(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
) -> std::io::Result<()> {
    let last = app
        .compose
        .as_ref()
        .is_some_and(|state| state.at_last_field());
    if last {
        return open_compose_editor(terminal, app, layout);
    }
    step_compose(app, 1);
    Ok(())
}

fn open_compose_editor(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    layout: &StoreLayout,
) -> std::io::Result<()> {
    if let Some(state) = app.compose.as_mut() {
        state.close_completion();
    }
    session::open_body_editor(terminal, app, layout)
}

/// The keymap context for a view, so a key resolves against
/// that surface's bindings before the Global fallback.
fn context_for(app: &App) -> Context {
    match app.view {
        View::List => Context::List,
        View::Pager | View::Image => Context::Pager,
        View::Review => Context::Review,
        View::Compose | View::Editor => Context::Compose,
        View::Settings => settings_context(app),
    }
}

fn settings_context(app: &App) -> Context {
    match app.settings.as_ref().map(|state| state.tab) {
        Some(SettingsTab::Accounts) => Context::SettingsAccounts,
        Some(SettingsTab::Essentials) => Context::SettingsEssentials,
        Some(SettingsTab::Folders) => Context::SettingsFolders,
        None => Context::Settings,
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
    if matches!(app.view, View::Pager | View::List) && app.drawer_open {
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
    let key_context = context_for(app);
    let Resolution::Match(action) = keymap.feed(key_context, key)
    else {
        return;
    };
    if read_only_blocks(app, action) {
        return;
    }
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
    if action == antiphon_core::Action::Sync {
        app.notice = Some(request_sync());
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

/// Whether the embedded editor is the surface under the mouse:
/// the full editor view, or the compose view with the pty pane
/// shown below the header fields.
fn editor_focused(app: &App) -> bool {
    app.editor.is_some()
        && matches!(app.view, View::Editor | View::Compose)
}

/// Hands a wheel notch over the editor pane to the editor as an
/// SGR mouse report, so it scrolls natively. Returns whether
/// the editor took the event; anything it declines falls back
/// to the pager. Nothing is forwarded unless the editor has
/// asked for mouse reporting, so an editor with the mouse off
/// is never fed stray escape bytes.
pub(super) fn editor_mouse(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mouse: MouseEvent,
) -> std::io::Result<bool> {
    let wheel = matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    );
    if !editor_focused(app) || !wheel {
        return Ok(false);
    }
    let size = terminal.size()?;
    let area =
        ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let Some((col, row)) =
        pane_relative(draw::editor_pane_area(area), mouse)
    else {
        return Ok(false);
    };
    let Some(pane) = app.editor.as_mut() else {
        return Ok(false);
    };
    if !pane.session.wants_mouse() {
        return Ok(false);
    }
    pane.session.send_mouse(mouse.kind, col, row);
    Ok(true)
}

/// Translates an absolute mouse position into the pane's own
/// zero-based coordinates, or None when it falls outside.
fn pane_relative(
    pane: ratatui::layout::Rect,
    mouse: MouseEvent,
) -> Option<(u16, u16)> {
    let inside = mouse.column >= pane.x
        && mouse.column < pane.x + pane.width
        && mouse.row >= pane.y
        && mouse.row < pane.y + pane.height;
    if !inside {
        return None;
    }
    Some((mouse.column - pane.x, mouse.row - pane.y))
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
    if let Some(index) =
        pager_body::image_index_at(app, chrome.body, column, row)
    {
        open_pager_image(app, index);
        return;
    }
    let Some(url) =
        pager_body::link_url_at(app, chrome.body, column, row)
    else {
        return;
    };
    link_picker::open_url(app, &url);
}

fn open_pager_image(app: &mut App, index: usize) {
    let Some(image) = app.pager_images.get(index) else {
        return;
    };
    let name = image.name.clone();
    let bytes = image.bytes.clone();
    app.open_image_view(name, &bytes);
}

pub(super) fn prompt_key(
    app: &mut App,
    keymap: &mut Keymap,
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
    if app
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.kind == PromptKind::ConfirmDelete)
    {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                app.prompt = None;
                app.delete_selected_forever();
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.prompt = None
            }
            _ => {}
        }
        return;
    }
    if app
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.kind == PromptKind::ConfirmBulk)
    {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                app.prompt = None;
                bulk::confirm(app);
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.prompt = None;
                bulk::cancel(app);
            }
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
    match keymap.feed(Context::Prompt, key) {
        Resolution::Match(Action::PromptSubmit) => {
            submit_prompt(app, layout)
        }
        Resolution::Match(Action::PromptCancel) => app.prompt_cancel(),
        _ => match key.code {
            KeyCode::Char(ch) => app.prompt_push(ch),
            KeyCode::Backspace => app.prompt_backspace(),
            _ => {}
        },
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
            export::run_pending(app, layout);
            bulk::run_pending(app, layout);
        }
        PromptKind::Search => run_search(app, layout, prompt.buffer),
        PromptKind::AttachmentPath => {
            add_attachment(app, &prompt.buffer)
        }
        PromptKind::SaveAttachment => {
            drawer::save_selected(app, &prompt.buffer)
        }
        PromptKind::ConfirmUnsubscribe
        | PromptKind::ConfirmDraft
        | PromptKind::ConfirmDelete
        | PromptKind::ConfirmBulk => {}
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

/// s asks the daemon for a pass right now; the reply is
/// bounded so a busy daemon reads as busy, never as a hang.
fn request_sync() -> String {
    use antiphon_ipc::{IpcClient, Request, Response, socket_path};

    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        return "sync: antiphond is not running".to_string();
    };
    let _ = client.set_read_timeout(super::IPC_WAIT);
    match client.request(&Request::SyncNow) {
        Ok(Response::Ack) => "sync requested".to_string(),
        Ok(Response::Error(error)) => format!("sync: {error}"),
        Ok(_) => "sync: unexpected daemon reply".to_string(),
        Err(_) => "sync: the daemon is busy; it will pass by itself"
            .to_string(),
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

#[cfg(test)]
mod tests {
    use super::super::testkit::app_with_messages;
    use super::*;

    #[test]
    fn read_only_blocks_mutations_with_a_notice() {
        let mut app = app_with_messages(1);
        assert!(!read_only_blocks(&mut app, Action::DeleteMessage));
        app.read_only = true;
        for action in MUTATING_ACTIONS {
            app.notice = None;
            assert!(read_only_blocks(&mut app, action), "{action:?}");
            assert_eq!(
                app.notice.as_deref(),
                Some(READ_ONLY_NOTICE),
                "{action:?}"
            );
        }
        assert!(app.pending_ops.is_empty(), "nothing was queued");
        assert!(!read_only_blocks(&mut app, Action::MoveDown));
        assert!(!read_only_blocks(&mut app, Action::Open));
    }

    fn wheel_at(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn pane_relative_offsets_inside_and_rejects_outside() {
        let pane = ratatui::layout::Rect::new(0, 5, 80, 10);
        assert_eq!(pane_relative(pane, wheel_at(10, 7)), Some((10, 2)));
        assert_eq!(pane_relative(pane, wheel_at(0, 5)), Some((0, 0)));
        // The row above the pane belongs to the header summary.
        assert_eq!(pane_relative(pane, wheel_at(10, 4)), None);
        // One past the bottom edge is outside.
        assert_eq!(pane_relative(pane, wheel_at(10, 15)), None);
    }
}
