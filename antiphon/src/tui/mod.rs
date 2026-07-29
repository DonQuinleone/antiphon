mod account_form;
mod account_form_draw;
mod account_form_fields;
mod account_form_identity;
mod account_form_identity_draw;
mod account_form_save;
mod actions;
mod app;
mod app_sidebar;
mod attach;
mod cells;
mod commands;
mod complete;
mod compose;
mod compose_assembly;
pub(crate) mod configedit;
mod crypto;
mod daemon;
mod decrypt;
mod dispatch;
mod drafts;
mod draw;
mod drawer;
mod editor;
mod export;
mod folder_alias;
mod folder_picker;
mod folders;
mod headers;
mod headers_draw;
mod help;
mod identity;
mod image_view;
mod input;
mod link_picker;
mod lists;
mod mailops;
mod mark_all_read;
mod message_list;
mod oauth_status;
mod oauthflow;
mod oauthflow_worker;
mod pager;
mod pager_actions;
mod pager_body;
mod pager_style;
mod patches;
mod prefill;
mod prefs;
mod preview;
mod reader;
mod replies;
mod review;
mod schedule;
mod scope;
mod session;
mod settings;
mod sidebar;
mod sidebar_draw;
mod status;
mod tabs;
#[cfg(test)]
mod testkit;
mod themecmd;
mod thread_tree;
mod thread_tree_draw;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use antiphon_config::{Dirs, Loaded};
use antiphon_core::Keymap;
use antiphon_store::{
    MessageSummary, SearchError, SearchIndex, StoreLayout,
};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;

use antiphon_pgp::Keyring;

use actions::account_names;
use app::{App, DEFAULT_QUERY, KeyRoute, View};
use daemon::{nudge_daemon, reload_in_background, request_reload};
use dispatch::{
    pending_resume_request, pending_rsvp_request,
    pending_template_request, pending_unsubscribe_request,
};
use identity::ComposeContext;
use scope::ViewScope;

const INPUT_POLL: Duration = Duration::from_millis(250);
const EDITOR_POLL: Duration = Duration::from_millis(20);
/// A busy daemon (mid sync pass) answers nothing; the UI must
/// never hang on it. Queued work is durable either way.
const IPC_WAIT: Duration = Duration::from_secs(2);
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// The auth-failure report changes rarely, so its Status
/// request rides the refresh tick at a much lower cadence.
const AUTH_POLL_EVERY: Duration = Duration::from_secs(30);
/// A notice that nothing replaces fades on its own, so
/// "sending: ..." cannot outlive the send it described.
const NOTICE_TTL: Duration = Duration::from_secs(8);
const LIST_WINDOW: usize = 500;
const UNREAD_QUERY: &str = "tag:unread";
const PGP_KEYRING_DIR: &str = "pgp";
const CONFIG_FILE_NAME: &str = "config.toml";

const THEMES_DIR: &str = "themes";

pub fn run(
    loaded: &Loaded,
    layout: &StoreLayout,
    dirs: &Dirs,
    read_only: bool,
    startup_notice: Option<String>,
) -> ExitCode {
    if let Err(error) =
        antiphon_ui::load_themes(&dirs.config.join(THEMES_DIR))
    {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
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
    let folders =
        sidebar::discover(layout, &prefs::account_seeds(loaded));
    let mut app = App::new(
        loaded,
        &folders,
        messages,
        total,
        keyring,
        dirs.config.join(CONFIG_FILE_NAME),
        dirs,
    );
    // Startup opens the default sidebar entry (the first
    // inbox), so the list shows it rather than highlighting
    // it over an unrelated query.
    app.read_only = read_only;
    app.notice = startup_notice;
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
    grab_mouse();
    let picker = image_view::make_picker();
    let outcome = event_loop(
        &mut terminal,
        &mut app,
        keymap,
        layout,
        &context,
        picker,
    );
    release_mouse();
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
    mut picker: ratatui_image::picker::Picker,
) -> std::io::Result<()> {
    let mut last_refresh = Instant::now();
    let mut last_unread: Option<u32> = None;
    let mut last_auth_poll: Option<Instant> = None;
    let mut notice_seen: Option<(String, Instant)> = None;
    while !app.quit {
        expire_notice(app, &mut notice_seen);
        oauthflow::poll(app);
        tick_editor(terminal, app)?;
        preview::refresh(app);
        prepare_image(terminal, app, &mut picker)?;
        let drawing = Instant::now();
        terminal.draw(|frame| draw::draw(frame, app))?;
        app.frame_stats.record(drawing.elapsed());
        if !event::poll(poll_interval(app))? {
            daemon::drain_ops(app);
            maybe_refresh(
                app,
                layout,
                &mut last_refresh,
                &mut last_unread,
            );
            maybe_poll_auth(app, &mut last_auth_poll);
            continue;
        }
        let event = event::read()?;
        if let Event::Mouse(mouse) = event {
            input::pager_mouse(terminal, app, mouse)?;
            daemon::drain_ops(app);
            continue;
        }
        let Event::Key(key) = event else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.key_route() {
            KeyRoute::Editor => input::editor_key(app, key),
            KeyRoute::Compose => {
                input::compose_key(terminal, app, layout, key)?
            }
            KeyRoute::Review => input::review_key(
                terminal,
                app,
                &mut keymap,
                layout,
                key,
            )?,
            KeyRoute::Settings => input::settings_key(app, key),
            KeyRoute::Prompt => {
                input::prompt_key(app, layout, key);
                let mut request =
                    pending_template_request(app, context);
                if request.is_none() {
                    request = pending_unsubscribe_request(app, context);
                }
                if request.is_none() {
                    request = pending_resume_request(app, context);
                }
                if request.is_none() {
                    request = pending_rsvp_request(app, context);
                }
                if let Some(state) = request {
                    app.start_compose(state);
                }
            }
            KeyRoute::Keymap => input::keymap_key(
                app,
                &mut keymap,
                layout,
                context,
                key,
            ),
        }
        daemon::drain_ops(app);
    }
    Ok(())
}

/// Every suspension of the TUI must release the mouse too:
/// leaving capture armed sprays SGR escape codes into
/// whatever reads the terminal next, as the account wizard
/// found out.
pub(super) fn grab_mouse() {
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
}

pub(super) fn release_mouse() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
}

fn expire_notice(app: &mut App, seen: &mut Option<(String, Instant)>) {
    let Some(current) = app.notice.clone() else {
        *seen = None;
        return;
    };
    match seen {
        Some((text, since)) if *text == current => {
            if since.elapsed() >= NOTICE_TTL {
                app.notice = None;
                *seen = None;
            }
        }
        _ => *seen = Some((current, Instant::now())),
    }
}

/// Encodes the open image for the current terminal size before
/// the frame that draws it; a graphics fault becomes a notice
/// rather than a torn frame.
fn prepare_image(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    picker: &mut ratatui_image::picker::Picker,
) -> std::io::Result<()> {
    if app.view != View::Image {
        return Ok(());
    }
    let size = terminal.size()?;
    let error = app
        .image_view
        .as_mut()
        .and_then(|view| image_view::prepare(view, picker, size));
    if let Some(error) = error {
        app.notice = Some(format!("image render: {error}"));
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
    last_refresh: &mut Instant,
    last_unread: &mut Option<u32>,
) {
    if last_refresh.elapsed() < REFRESH_EVERY {
        return;
    }
    *last_refresh = Instant::now();
    app.sync_progress = antiphon_sync::read_progress(layout);
    app.account_entries =
        sidebar::discover(layout, &app.account_entries);
    let index = SearchIndex::open(layout).ok();
    let mut entries = app.build_sidebar_entries();
    if let Some(index) = &index {
        sidebar::fill_unread(&mut entries, |query| {
            index.count(&format!("tag:unread and ({query})")).ok()
        });
    }
    app.update_sidebar(entries);
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
    let Some(index) = index else {
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
    app.clamp_selected_visible();
}

/// Refreshes the daemon's auth-failure report for the status
/// line; silent when the daemon is down, so the last answer
/// stands rather than flapping.
fn maybe_poll_auth(app: &mut App, last: &mut Option<Instant>) {
    if app.read_only {
        return;
    }
    if last.is_some_and(|at| at.elapsed() < AUTH_POLL_EVERY) {
        return;
    }
    *last = Some(Instant::now());
    if let Some(failures) = daemon::auth_failures() {
        app.auth_failures = failures;
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
