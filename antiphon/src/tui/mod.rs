mod app;
mod draw;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use antiphon_config::Loaded;
use antiphon_core::{Action, Keymap, Resolution};
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

use app::{App, DEFAULT_QUERY, OpIntent, PromptKind, View};

const INPUT_POLL: Duration = Duration::from_millis(250);
const REFRESH_EVERY: Duration = Duration::from_secs(2);
const LIST_WINDOW: usize = 500;
const DAEMON_ASSIGNS_ID: u64 = 0;
const UNREAD_QUERY: &str = "tag:unread";

pub fn run(loaded: &Loaded, layout: &StoreLayout) -> ExitCode {
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
    let mut app = App::new(loaded, messages, total);
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, &mut app, keymap, layout);
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
) -> std::io::Result<()> {
    let mut last_refresh = Instant::now();
    let mut last_unread: Option<u32> = None;
    while !app.quit {
        terminal.draw(|frame| draw::draw(frame, app))?;
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
            dispatch(app, action);
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

fn dispatch(app: &mut App, action: Action) {
    let opening = action == Action::Open && app.view == View::List;
    if !opening {
        app.apply(action);
        return;
    }
    let Some(message) = app.selected_message() else {
        return;
    };
    match std::fs::read(&message.path) {
        Ok(raw) => app.open_pager(body_text(&raw)),
        Err(error) => {
            app.open_pager(format!(
                "cannot read {}: {error}",
                message.path.display()
            ));
        }
    }
}

fn body_text(raw: &[u8]) -> String {
    antiphon_render::body_text(raw).text
}
