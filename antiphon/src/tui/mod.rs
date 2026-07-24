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
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use app::{App, View};

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
    let (messages, total) = match load_messages(layout) {
        Ok(loaded_messages) => loaded_messages,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("run `antiphon doctor` to check the setup");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(loaded, messages, total);
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, &mut app, keymap);
    ratatui::restore();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("terminal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_messages(
    layout: &StoreLayout,
) -> Result<(Vec<MessageSummary>, u32), SearchError> {
    let index = SearchIndex::open(layout)?;
    let messages = index.query("*", Some(LIST_WINDOW))?;
    let total = index.count("*")?;
    Ok((messages, total))
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut keymap: Keymap,
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
        if let Resolution::Match(action) = keymap.feed(key) {
            dispatch(app, action);
        }
    }
    Ok(())
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
