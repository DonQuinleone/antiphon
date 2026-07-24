use antiphon_config::{Loaded, ReadingPane};
use antiphon_core::Action;
use antiphon_store::MessageSummary;
use antiphon_ui::{Theme, VESPERS};

const HALF_PAGE_ROWS: usize = 10;
const PAGER_SCROLL_ROWS: u16 = 1;
const PAGER_HALF_PAGE_ROWS: u16 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    List,
    Pager,
}

pub struct App {
    pub accounts: Vec<String>,
    pub messages: Vec<MessageSummary>,
    pub total_messages: u32,
    pub selected: usize,
    pub view: View,
    pub pager_body: String,
    pub pager_scroll: u16,
    pub reading_pane: ReadingPane,
    pub sidebar: bool,
    pub theme: &'static Theme,
    pub date_format: String,
    pub notice: Option<&'static str>,
    pub quit: bool,
}

impl App {
    pub fn new(
        loaded: &Loaded,
        messages: Vec<MessageSummary>,
        total_messages: u32,
    ) -> App {
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
            total_messages,
            selected: 0,
            view: View::List,
            pager_body: String::new(),
            pager_scroll: 0,
            reading_pane: loaded.config.ui.reading_pane,
            sidebar: true,
            theme,
            date_format: loaded.config.ui.date_format.clone(),
            notice: None,
            quit: false,
        }
    }

    pub fn open_pager(&mut self, body: String) {
        self.pager_body = body;
        self.pager_scroll = 0;
        self.view = View::Pager;
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
        match self.view {
            View::List => self.apply_in_list(action),
            View::Pager => self.apply_in_pager(action),
        }
    }

    fn apply_in_list(&mut self, action: Action) {
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
            _ => {
                self.notice =
                    Some("not built yet; see DESIGN.md milestones")
            }
        }
    }

    fn apply_in_pager(&mut self, action: Action) {
        match action {
            Action::MoveDown => {
                self.scroll_pager(PAGER_SCROLL_ROWS as i32)
            }
            Action::MoveUp => {
                self.scroll_pager(-(PAGER_SCROLL_ROWS as i32))
            }
            Action::HalfPageDown => {
                self.scroll_pager(PAGER_HALF_PAGE_ROWS as i32)
            }
            Action::HalfPageUp => {
                self.scroll_pager(-(PAGER_HALF_PAGE_ROWS as i32))
            }
            Action::Top => self.pager_scroll = 0,
            Action::Back | Action::Quit => self.view = View::List,
            _ => {
                self.notice =
                    Some("not built yet; see DESIGN.md milestones")
            }
        }
    }

    fn scroll_pager(&mut self, rows: i32) {
        let scrolled = i32::from(self.pager_scroll) + rows;
        let ceiling = self.pager_line_count();
        self.pager_scroll = scrolled.clamp(0, ceiling) as u16;
    }

    fn pager_line_count(&self) -> i32 {
        self.pager_body.lines().count() as i32
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
                path: std::path::PathBuf::new(),
            })
            .collect();
        App {
            accounts: Vec::new(),
            messages,
            total_messages: count as u32,
            selected: 0,
            view: View::List,
            pager_body: String::new(),
            pager_scroll: 0,
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
    fn pager_scrolls_clamped_and_returns_to_the_list() {
        let mut app = app_with_messages(1);
        app.open_pager("one\ntwo\nthree\n".to_string());
        assert_eq!(app.view, View::Pager);
        app.apply(Action::MoveUp);
        assert_eq!(app.pager_scroll, 0);
        app.apply(Action::HalfPageDown);
        assert_eq!(app.pager_scroll, 3);
        app.apply(Action::Top);
        assert_eq!(app.pager_scroll, 0);
        app.apply(Action::Quit);
        assert_eq!(app.view, View::List);
        assert!(!app.quit);
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
