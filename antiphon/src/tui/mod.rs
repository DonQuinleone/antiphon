mod app;
mod draw;

use std::process::ExitCode;
use std::time::Duration;

use antiphon_config::Loaded;
use antiphon_core::{Keymap, Resolution};
use antiphon_store::{
    MessageSummary, SearchError, SearchIndex, StoreLayout,
};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use app::App;

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
    let messages = match load_messages(layout) {
        Ok(messages) => messages,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("run `antiphon doctor` to check the setup");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(loaded, messages);
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
) -> Result<Vec<MessageSummary>, SearchError> {
    let index = SearchIndex::open(layout)?;
    index.query("*", Some(LIST_WINDOW))
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
            app.apply(action);
        }
    }
    Ok(())
}
