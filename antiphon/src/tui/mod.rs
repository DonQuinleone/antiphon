mod app;
mod draw;

use std::process::ExitCode;
use std::time::Duration;

use antiphon_config::Loaded;
use antiphon_core::{Action, Keymap, Resolution};
use antiphon_store::{
    MessageSummary, SearchError, SearchIndex, StoreLayout,
};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind,
};

use app::{App, DEFAULT_QUERY, PromptKind, View};

const INPUT_POLL: Duration = Duration::from_millis(250);
const LIST_WINDOW: usize = 500;

pub fn run(loaded: &Loaded, layout: &StoreLayout) -> ExitCode {
    let keymap = match Keymap::new(&loaded.config.keys) {
        Ok(keymap) => keymap,
        Err(error) => {
            eprintln!("keymap: {error}");
            return ExitCode::FAILURE;
        }
    };
    let index = match SearchIndex::open(layout) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("run `antiphon doctor` to check the setup");
            return ExitCode::FAILURE;
        }
    };
    let (messages, total) = match query_window(&index, DEFAULT_QUERY) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(loaded, messages, total);
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, &mut app, keymap, &index);
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
    index: &SearchIndex,
    query: &str,
) -> Result<(Vec<MessageSummary>, u32), SearchError> {
    let messages = index.query(query, Some(LIST_WINDOW))?;
    let total = index.count(query)?;
    Ok((messages, total))
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut keymap: Keymap,
    index: &SearchIndex,
) -> std::io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| draw::draw(frame, app))?;
        if !event::poll(INPUT_POLL)? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if app.prompt.is_some() {
            prompt_key(app, index, key);
            continue;
        }
        if let Resolution::Match(action) = keymap.feed(key) {
            dispatch(app, action);
        }
    }
    Ok(())
}

fn prompt_key(app: &mut App, index: &SearchIndex, key: KeyEvent) {
    match key.code {
        KeyCode::Char(ch) => app.prompt_push(ch),
        KeyCode::Backspace => app.prompt_backspace(),
        KeyCode::Esc => app.prompt_cancel(),
        KeyCode::Enter => submit_prompt(app, index),
        _ => {}
    }
}

fn submit_prompt(app: &mut App, index: &SearchIndex) {
    let Some(prompt) = app.prompt_submit() else {
        return;
    };
    match prompt.kind {
        PromptKind::Command => app.run_command(&prompt.buffer),
        PromptKind::Search => run_search(app, index, prompt.buffer),
    }
}

fn run_search(app: &mut App, index: &SearchIndex, raw: String) {
    let query = if raw.trim().is_empty() {
        DEFAULT_QUERY.to_string()
    } else {
        raw
    };
    match query_window(index, &query) {
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
