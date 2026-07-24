use antiphon_config::{Loaded, ReadingPane};
use antiphon_core::Action;
use antiphon_store::MessageSummary;
use antiphon_ui::{Theme, VESPERS};

const HALF_PAGE_ROWS: usize = 10;

pub struct App {
    pub accounts: Vec<String>,
    pub messages: Vec<MessageSummary>,
    pub selected: usize,
    pub reading_pane: ReadingPane,
    pub sidebar: bool,
    pub theme: &'static Theme,
    pub date_format: String,
    pub notice: Option<&'static str>,
    pub quit: bool,
}

impl App {
    pub fn new(loaded: &Loaded, messages: Vec<MessageSummary>) -> App {
        let accounts = loaded
            .accounts
            .iter()
            .map(|entry| entry.account.account.name.clone())
            .collect();
        let theme =
            Theme::by_name(&loaded.config.ui.theme).unwrap_or(&VESPERS);
        App {
            accounts,
            messages,
            selected: 0,
            reading_pane: loaded.config.ui.reading_pane,
            sidebar: true,
            theme,
            date_format: loaded.config.ui.date_format.clone(),
            notice: None,
            quit: false,
        }
    }

    pub fn selected_message(&self) -> Option<&MessageSummary> {
        self.messages.get(self.selected)
    }

    pub fn unread_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|message| message.unread)
            .count()
    }

    pub fn apply(&mut self, action: Action) {
        self.notice = None;
        match action {
            Action::MoveDown => self.select_forward(1),
            Action::MoveUp => self.select_back(1),
            Action::HalfPageDown => self.select_forward(HALF_PAGE_ROWS),
            Action::HalfPageUp => self.select_back(HALF_PAGE_ROWS),
            Action::Top => self.selected = 0,
            Action::Bottom => self.selected = self.last_index(),
            Action::ToggleSidebar => self.sidebar = !self.sidebar,
            Action::CycleReadingPane => self.cycle_reading_pane(),
            Action::Quit => self.quit = true,
            _ => self.notice = Some("arrives later in M2"),
        }
    }

    fn select_forward(&mut self, rows: usize) {
        self.selected = (self.selected + rows).min(self.last_index());
    }

    fn select_back(&mut self, rows: usize) {
        self.selected = self.selected.saturating_sub(rows);
    }

    fn last_index(&self) -> usize {
        self.messages.len().saturating_sub(1)
    }

    fn cycle_reading_pane(&mut self) {
        self.reading_pane = match self.reading_pane {
            ReadingPane::Below => ReadingPane::Right,
            ReadingPane::Right => ReadingPane::Off,
            ReadingPane::Off => ReadingPane::Below,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_messages(count: usize) -> App {
        let messages = (0..count)
            .map(|index| MessageSummary {
                id: format!("m{index}"),
                thread_id: String::new(),
                subject: String::new(),
                from: String::new(),
                date_unix: index as i64,
                tags: Vec::new(),
                unread: index % 2 == 0,
            })
            .collect();
        App {
            accounts: Vec::new(),
            messages,
            selected: 0,
            reading_pane: ReadingPane::Below,
            sidebar: true,
            theme: &VESPERS,
            date_format: String::new(),
            notice: None,
            quit: false,
        }
    }

    #[test]
    fn movement_clamps_at_both_ends() {
        let mut app = app_with_messages(3);
        app.apply(Action::MoveUp);
        assert_eq!(app.selected, 0);
        app.apply(Action::Bottom);
        assert_eq!(app.selected, 2);
        app.apply(Action::MoveDown);
        assert_eq!(app.selected, 2);
        app.apply(Action::HalfPageUp);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn half_page_moves_by_the_constant() {
        let mut app = app_with_messages(30);
        app.apply(Action::HalfPageDown);
        assert_eq!(app.selected, HALF_PAGE_ROWS);
    }

    #[test]
    fn reading_pane_cycles_through_all_three() {
        let mut app = app_with_messages(1);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Right);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Off);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Below);
    }

    #[test]
    fn unhandled_actions_leave_a_notice_and_quit_quits() {
        let mut app = app_with_messages(1);
        app.apply(Action::Compose);
        assert!(app.notice.is_some());
        app.apply(Action::Quit);
        assert!(app.quit);
    }

    #[test]
    fn empty_list_never_panics() {
        let mut app = app_with_messages(0);
        app.apply(Action::Bottom);
        app.apply(Action::MoveDown);
        assert_eq!(app.selected, 0);
        assert!(app.selected_message().is_none());
    }
}
