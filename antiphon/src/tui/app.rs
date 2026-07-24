use antiphon_config::{Composer, Loaded, ReadingPane};
use antiphon_core::Action;
use antiphon_store::MessageSummary;
use antiphon_ui::{Theme, VESPERS};

use super::editor::EditorPane;
use super::scope::{self, ViewScope};
use super::scope_shim::ScopeError;
use super::sidebar::{self, SidebarEntry};

const HALF_PAGE_ROWS: usize = 10;
const PAGER_SCROLL_ROWS: u16 = 1;
const PAGER_HALF_PAGE_ROWS: u16 = 10;

const UNREAD_TAG: &str = "unread";
const TEMPLATE_COMMAND: &str = "template ";
pub const DEFAULT_QUERY: &str = "*";
const FLAGGED_TAG: &str = "flagged";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    List,
    Pager,
    Editor,
}

/// Where the next key event goes; resolved before the keymap
/// so an open editor swallows everything raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    Prompt,
    Editor,
    Keymap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    pub frames: u64,
    pub last_micros: u128,
    pub max_micros: u128,
    total_micros: u128,
}

impl FrameStats {
    pub fn record(&mut self, elapsed: std::time::Duration) {
        let micros = elapsed.as_micros();
        self.frames += 1;
        self.last_micros = micros;
        self.max_micros = self.max_micros.max(micros);
        self.total_micros += micros;
    }

    pub fn mean_micros(&self) -> u128 {
        if self.frames == 0 {
            return 0;
        }
        self.total_micros / u128::from(self.frames)
    }

    pub fn summary(&self) -> String {
        format!(
            "frames: {} drawn, last {} us, mean {} us, max {} us",
            self.frames,
            self.last_micros,
            self.mean_micros(),
            self.max_micros,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Search,
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub buffer: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpIntent {
    Flag {
        account: String,
        message_id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Delete {
        account: String,
        message_id: String,
    },
}

pub fn account_of(path: &std::path::Path) -> String {
    let mut components = path.components();
    for component in components.by_ref() {
        if component.as_os_str() == "maildir" {
            break;
        }
    }
    components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn account_names(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .map(|entry| entry.account.account.name.clone())
        .collect()
}

pub struct App {
    pub accounts: Vec<String>,
    pub scope: ViewScope,
    pub sidebar_entries: Vec<SidebarEntry>,
    pub sidebar_selected: usize,
    pub active_search: Option<String>,
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
    pub notice: Option<String>,
    pub prompt: Option<Prompt>,
    pub current_query: String,
    pub pending_ops: Vec<OpIntent>,
    pub pending_template: Option<String>,
    pub frame_stats: FrameStats,
    pub composer: Composer,
    pub editor: Option<EditorPane>,
    editor_return: View,
    requery: bool,
    pub quit: bool,
}

impl App {
    pub fn new(
        loaded: &Loaded,
        messages: Vec<MessageSummary>,
        total_messages: u32,
    ) -> App {
        let accounts = account_names(loaded);
        let sidebar_entries =
            sidebar::entries(&accounts, &loaded.config.saved_searches);
        let theme =
            Theme::by_name(&loaded.config.ui.theme).unwrap_or(&VESPERS);
        App {
            accounts,
            scope: ViewScope::Unified,
            sidebar_entries,
            sidebar_selected: 0,
            active_search: None,
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
            prompt: None,
            current_query: DEFAULT_QUERY.to_string(),
            pending_ops: Vec::new(),
            pending_template: None,
            frame_stats: FrameStats::default(),
            composer: loaded.config.ui.composer,
            editor: None,
            editor_return: View::List,
            requery: false,
            quit: false,
        }
    }

    /// Every query the client runs is built here, so nothing
    /// can reach the index without the scope conjoined.
    pub fn scoped(
        &self,
        user_query: &str,
    ) -> Result<String, ScopeError> {
        scope::effective_query(&self.scope, &self.accounts, user_query)
    }

    pub fn take_requery(&mut self) -> bool {
        std::mem::take(&mut self.requery)
    }

    pub fn key_route(&self) -> KeyRoute {
        if self.prompt.is_some() {
            return KeyRoute::Prompt;
        }
        if self.view == View::Editor {
            return KeyRoute::Editor;
        }
        KeyRoute::Keymap
    }

    pub fn open_editor(&mut self, pane: EditorPane) {
        self.editor_return = self.view;
        self.editor = Some(pane);
        self.view = View::Editor;
    }

    pub fn close_editor(&mut self) -> Option<EditorPane> {
        let pane = self.editor.take()?;
        self.view = self.editor_return;
        Some(pane)
    }

    pub fn open_pager(&mut self, body: String) {
        self.set_unread(false);
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
            View::Editor => {}
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
            Action::NextAccount => self.shift_scope(scope::next_scope),
            Action::PreviousAccount => {
                self.shift_scope(scope::previous_scope)
            }
            Action::SidebarNext => {
                self.sidebar_selected = sidebar::next_index(
                    self.sidebar_selected,
                    self.sidebar_entries.len(),
                )
            }
            Action::SidebarPrevious => {
                self.sidebar_selected = sidebar::previous_index(
                    self.sidebar_selected,
                    self.sidebar_entries.len(),
                )
            }
            Action::SidebarOpen => self.sidebar_open(),
            Action::CycleReadingPane => self.cycle_reading_pane(),
            Action::Search => self.open_prompt(PromptKind::Search),
            Action::Command => self.open_prompt(PromptKind::Command),
            Action::MarkRead => self.set_unread(false),
            Action::MarkUnread => self.set_unread(true),
            Action::ToggleFlagged => self.toggle_flagged(),
            Action::DeleteMessage => self.delete_selected(),
            Action::Quit => self.quit = true,
            _ => self.not_built_notice(),
        }
    }

    fn shift_scope(
        &mut self,
        step: fn(&ViewScope, &[String]) -> ViewScope,
    ) {
        self.scope = step(&self.scope, &self.accounts);
        self.requery = true;
    }

    fn sidebar_open(&mut self) {
        let Some(entry) =
            self.sidebar_entries.get(self.sidebar_selected)
        else {
            return;
        };
        match entry.clone() {
            SidebarEntry::Unified => self.scope = ViewScope::Unified,
            SidebarEntry::Account(account) => {
                self.scope = ViewScope::Account(account)
            }
            SidebarEntry::Saved { name, query } => {
                self.current_query = query;
                self.active_search = Some(name);
            }
        }
        self.requery = true;
    }

    fn open_prompt(&mut self, kind: PromptKind) {
        self.prompt = Some(Prompt {
            kind,
            buffer: String::new(),
        });
    }

    pub fn prompt_push(&mut self, ch: char) {
        if let Some(prompt) = &mut self.prompt {
            prompt.buffer.push(ch);
        }
    }

    pub fn prompt_backspace(&mut self) {
        if let Some(prompt) = &mut self.prompt {
            prompt.buffer.pop();
        }
    }

    pub fn prompt_cancel(&mut self) {
        self.prompt = None;
    }

    pub fn prompt_submit(&mut self) -> Option<Prompt> {
        self.prompt.take()
    }

    pub fn set_results(
        &mut self,
        messages: Vec<MessageSummary>,
        total: u32,
        query: String,
    ) {
        self.messages = messages;
        self.total_messages = total;
        self.selected = 0;
        self.current_query = query;
    }

    pub fn run_command(&mut self, command: &str) {
        match command.trim() {
            "q" | "quit" => self.quit = true,
            "frames" => self.notice = Some(self.frame_stats.summary()),
            other if other.starts_with(TEMPLATE_COMMAND) => {
                let name = other[TEMPLATE_COMMAND.len()..].trim();
                if name.is_empty() {
                    self.notice =
                        Some("usage: template <name>".to_string());
                } else {
                    self.pending_template = Some(name.to_string());
                }
            }
            "" => {}
            other => {
                self.notice = Some(format!("unknown command: {other}"))
            }
        }
    }

    fn not_built_notice(&mut self) {
        self.notice =
            Some("not built yet; see DESIGN.md milestones".to_string());
    }

    fn set_unread(&mut self, unread: bool) {
        let Some(message) = self.messages.get_mut(self.selected) else {
            return;
        };
        if message.unread == unread {
            return;
        }
        message.unread = unread;
        let tag = UNREAD_TAG.to_string();
        let (add, remove) = if unread {
            message.tags.push(tag.clone());
            (vec![tag], Vec::new())
        } else {
            message.tags.retain(|t| t != UNREAD_TAG);
            (Vec::new(), vec![tag])
        };
        self.pending_ops.push(OpIntent::Flag {
            account: account_of(&message.path),
            message_id: message.id.clone(),
            add,
            remove,
        });
    }

    fn toggle_flagged(&mut self) {
        let Some(message) = self.messages.get_mut(self.selected) else {
            return;
        };
        let tag = FLAGGED_TAG.to_string();
        let flagged = message.tags.iter().any(|t| t == FLAGGED_TAG);
        let (add, remove) = if flagged {
            message.tags.retain(|t| t != FLAGGED_TAG);
            (Vec::new(), vec![tag])
        } else {
            message.tags.push(tag.clone());
            (vec![tag], Vec::new())
        };
        self.pending_ops.push(OpIntent::Flag {
            account: account_of(&message.path),
            message_id: message.id.clone(),
            add,
            remove,
        });
    }

    fn delete_selected(&mut self) {
        if self.selected >= self.messages.len() {
            return;
        }
        let message = self.messages.remove(self.selected);
        self.total_messages = self.total_messages.saturating_sub(1);
        self.pending_ops.push(OpIntent::Delete {
            account: account_of(&message.path),
            message_id: message.id,
        });
        self.selected = self.selected.min(self.last_index());
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
            _ => self.not_built_notice(),
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
            scope: ViewScope::Unified,
            sidebar_entries: Vec::new(),
            sidebar_selected: 0,
            active_search: None,
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
            prompt: None,
            current_query: DEFAULT_QUERY.to_string(),
            pending_ops: Vec::new(),
            pending_template: None,
            frame_stats: FrameStats::default(),
            composer: Composer::Embedded,
            editor: None,
            editor_return: View::List,
            requery: false,
            quit: false,
        }
    }

    fn app_with_accounts(names: &[&str]) -> App {
        let mut app = app_with_messages(1);
        app.accounts =
            names.iter().map(|name| (*name).to_string()).collect();
        app.sidebar_entries = sidebar::entries(&app.accounts, &[]);
        app
    }

    #[test]
    fn keys_route_to_the_editor_view_before_the_keymap() {
        let mut app = app_with_messages(1);
        assert_eq!(app.key_route(), KeyRoute::Keymap);
        app.view = View::Editor;
        assert_eq!(app.key_route(), KeyRoute::Editor);
        app.view = View::List;
        app.apply(Action::Search);
        assert_eq!(app.key_route(), KeyRoute::Prompt);
    }

    #[test]
    fn editor_view_swallows_actions_unchanged() {
        let mut app = app_with_messages(1);
        app.view = View::Editor;
        app.apply(Action::Quit);
        assert!(!app.quit);
        assert_eq!(app.view, View::Editor);
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
        app.apply(Action::Sync);
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
    fn marking_read_flips_state_and_queues_one_op() {
        let mut app = app_with_messages(2);
        assert!(app.messages[0].unread);
        app.apply(Action::MarkRead);
        app.apply(Action::MarkRead);
        assert!(!app.messages[0].unread);
        assert_eq!(app.pending_ops.len(), 1);
        let OpIntent::Flag { remove, add, .. } = &app.pending_ops[0]
        else {
            panic!("expected a flag op");
        };
        assert_eq!(remove, &vec!["unread".to_string()]);
        assert!(add.is_empty());
    }

    #[test]
    fn opening_the_pager_marks_the_message_read() {
        let mut app = app_with_messages(1);
        app.open_pager(String::new());
        assert!(!app.messages[0].unread);
        assert_eq!(app.pending_ops.len(), 1);
    }

    #[test]
    fn flag_toggle_round_trips_through_tags() {
        let mut app = app_with_messages(1);
        app.apply(Action::ToggleFlagged);
        assert!(app.messages[0].tags.contains(&"flagged".into()));
        app.apply(Action::ToggleFlagged);
        assert!(!app.messages[0].tags.contains(&"flagged".into()));
        assert_eq!(app.pending_ops.len(), 2);
    }

    #[test]
    fn accounts_derive_from_maildir_paths() {
        let cases = [
            ("/store/maildir/work/cur/1.host:2,S", "work"),
            ("/store/maildir/personal/new/2.host", "personal"),
            ("/elsewhere/3.host", ""),
        ];
        for (path, expected) in cases {
            assert_eq!(
                account_of(std::path::Path::new(path)),
                expected,
                "{path}"
            );
        }
    }

    #[test]
    fn delete_removes_the_row_and_clamps_selection() {
        let mut app = app_with_messages(2);
        app.apply(Action::Bottom);
        app.apply(Action::DeleteMessage);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.selected, 0);
        assert_eq!(app.total_messages, 1);
        assert!(matches!(app.pending_ops[0], OpIntent::Delete { .. }));
    }

    #[test]
    fn prompt_edits_cancels_and_submits() {
        let mut app = app_with_messages(1);
        app.apply(Action::Search);
        for ch in "tag:unread".chars() {
            app.prompt_push(ch);
        }
        app.prompt_backspace();
        let prompt = app.prompt_submit().expect("open prompt");
        assert_eq!(prompt.kind, PromptKind::Search);
        assert_eq!(prompt.buffer, "tag:unrea");
        assert!(app.prompt.is_none());

        app.apply(Action::Command);
        app.prompt_cancel();
        assert!(app.prompt.is_none());
    }

    #[test]
    fn frame_stats_track_last_mean_and_max() {
        let mut stats = FrameStats::default();
        stats.record(std::time::Duration::from_micros(100));
        stats.record(std::time::Duration::from_micros(300));
        assert_eq!(stats.frames, 2);
        assert_eq!(stats.last_micros, 300);
        assert_eq!(stats.max_micros, 300);
        assert_eq!(stats.mean_micros(), 200);
        assert_eq!(FrameStats::default().mean_micros(), 0);
    }

    #[test]
    fn commands_quit_or_complain() {
        let mut app = app_with_messages(1);
        app.run_command("nonsense");
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("nonsense"))
        );
        app.run_command("q");
        assert!(app.quit);
    }

    #[test]
    fn results_replace_the_window_and_reset_selection() {
        let mut app = app_with_messages(5);
        app.apply(Action::Bottom);
        app.set_results(Vec::new(), 0, "tag:flagged".into());
        assert_eq!(app.selected, 0);
        assert_eq!(app.total_messages, 0);
        assert_eq!(app.current_query, "tag:flagged");
    }

    #[test]
    fn gt_cycles_unified_through_accounts_and_back() {
        let mut app = app_with_accounts(&["a", "b"]);
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Unified);
        app.apply(Action::PreviousAccount);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert!(!app.take_requery());
    }

    #[test]
    fn sidebar_moves_in_entry_order_without_querying() {
        let mut app = app_with_accounts(&["a"]);
        app.apply(Action::SidebarNext);
        app.apply(Action::SidebarNext);
        assert_eq!(app.sidebar_selected, 2);
        assert!(!app.take_requery());
        app.apply(Action::SidebarPrevious);
        assert_eq!(app.sidebar_selected, 1);
    }

    #[test]
    fn opening_an_account_entry_sets_the_scope() {
        let mut app = app_with_accounts(&["a", "b"]);
        app.apply(Action::SidebarNext);
        app.apply(Action::SidebarNext);
        app.apply(Action::SidebarOpen);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert_eq!(app.current_query, DEFAULT_QUERY);
        assert!(app.active_search.is_none());
    }

    #[test]
    fn opening_a_saved_search_keeps_scope_and_names_it() {
        let mut app = app_with_accounts(&["a"]);
        app.scope = ViewScope::Account("a".into());
        let unread = app
            .sidebar_entries
            .iter()
            .position(|entry| entry.label() == "unread")
            .expect("built-in unread entry");
        app.sidebar_selected = unread;
        app.apply(Action::SidebarOpen);
        assert_eq!(app.current_query, "tag:unread");
        assert_eq!(app.active_search.as_deref(), Some("unread"));
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
    }

    #[test]
    fn app_queries_are_always_scope_conjoined() {
        let mut app = app_with_accounts(&["a", "b"]);
        assert_eq!(
            app.scoped("tag:unread").unwrap(),
            "(path:\"a/**\" or path:\"b/**\") and (tag:unread)",
        );
        app.scope = ViewScope::Account("a".into());
        let scoped = app.scoped("*").unwrap();
        assert_eq!(scoped, "(path:\"a/**\")");
        assert!(!scoped.contains('b'));
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
