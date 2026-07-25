use antiphon_config::{Composer, Loaded, ReadingPane};
use antiphon_core::Action;
use antiphon_pgp::{Keyring, Signature};
use antiphon_render::MailtoUnsubscribe;
use antiphon_store::MessageSummary;
use antiphon_ui::{Theme, VESPERS};

use super::actions::{OpIntent, account_names};
use super::commands::{FrameStats, PatchCommand, Prompt};
use super::editor::EditorPane;
use super::scope::{self, ViewScope};
use super::sidebar::{self, AccountEntry, SidebarEntry};
use antiphon_store::ScopeError;

const PAGER_SCROLL_ROWS: u16 = 1;
const PAGER_HALF_PAGE_ROWS: u16 = 10;

pub const DEFAULT_QUERY: &str = "*";

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
    pub pager_patch: Vec<antiphon_render::PatchLine>,
    pub pager_signature: Signature,
    pub pager_invite: Vec<String>,
    pub pager_scroll: u16,
    pub pager_raw: Vec<u8>,
    pub pager_html: bool,
    pub preview_scroll: u16,
    pub keyring: Keyring,
    pub own_addresses: Vec<String>,
    pub preview: Option<super::preview::Preview>,
    pub reading_pane: ReadingPane,
    pub sidebar: bool,
    pub list_rows: u16,
    pub sidebar_width: u16,
    pub theme: &'static Theme,
    pub date_format: String,
    pub notice: Option<String>,
    pub prompt: Option<Prompt>,
    pub current_query: String,
    pub pending_ops: Vec<OpIntent>,
    pub sync_progress: Option<antiphon_sync::SyncProgress>,
    pub pending_template: Option<String>,
    pub pending_patches: Option<PatchCommand>,
    pub(super) pending_sign: Option<bool>,
    pub(super) pending_encrypt: Option<bool>,
    pub(super) pending_one_click: Option<String>,
    pub(super) pending_unsubscribe: Option<(String, MailtoUnsubscribe)>,
    pub frame_stats: FrameStats,
    pub composer: Composer,
    pub editor: Option<EditorPane>,
    editor_return: View,
    pub(super) requery: bool,
    pub quit: bool,
}

impl App {
    pub fn new(
        loaded: &Loaded,
        folders: &[AccountEntry],
        messages: Vec<MessageSummary>,
        total_messages: u32,
        keyring: Keyring,
    ) -> App {
        let accounts = account_names(loaded);
        let own_addresses = own_addresses(loaded);
        let sidebar_entries =
            sidebar::entries(folders, &loaded.config.saved_searches);
        let sidebar_selected =
            sidebar::default_selection(&sidebar_entries);
        let theme =
            Theme::by_name(&loaded.config.ui.theme).unwrap_or(&VESPERS);
        App {
            accounts,
            scope: ViewScope::Unified,
            sidebar_entries,
            sidebar_selected,
            active_search: Some(sidebar::ALL_LABEL.to_string()),
            messages,
            total_messages,
            selected: 0,
            view: View::List,
            pager_body: String::new(),
            pager_patch: Vec::new(),
            pager_signature: Signature::none(),
            pager_invite: Vec::new(),
            pager_scroll: 0,
            pager_raw: Vec::new(),
            pager_html: false,
            preview_scroll: 0,
            keyring,
            own_addresses,
            preview: None,
            reading_pane: loaded.config.ui.reading_pane,
            sidebar: true,
            list_rows: loaded.config.ui.list_rows,
            sidebar_width: loaded.config.ui.sidebar_width,
            theme,
            date_format: loaded.config.ui.date_format.clone(),
            notice: None,
            prompt: None,
            current_query: DEFAULT_QUERY.to_string(),
            pending_ops: Vec::new(),
            sync_progress: None,
            pending_template: None,
            pending_patches: None,
            pending_sign: None,
            pending_encrypt: None,
            pending_one_click: None,
            pending_unsubscribe: None,
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

    pub fn open_pager(
        &mut self,
        body: String,
        signature: Signature,
        invite: Vec<String>,
    ) {
        self.set_unread(false);
        self.pager_patch = patch_lines(self.selected_message(), &body);
        self.pager_body = body;
        self.pager_signature = signature;
        self.pager_invite = invite;
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

    /// Folders come and go as syncs land; the highlight is
    /// clamped rather than reset so it never dangles.
    pub fn update_sidebar(&mut self, entries: Vec<SidebarEntry>) {
        if entries == self.sidebar_entries {
            return;
        }
        let last = entries.len().saturating_sub(1);
        self.sidebar_entries = entries;
        self.sidebar_selected = self.sidebar_selected.min(last);
    }

    pub(super) fn not_built_notice(&mut self) {
        self.notice =
            Some("not built yet; see DESIGN.md milestones".to_string());
    }

    fn apply_in_pager(&mut self, action: Action) {
        match action {
            Action::MoveDown | Action::Open => {
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
}

fn patch_lines(
    selected: Option<&MessageSummary>,
    body: &str,
) -> Vec<antiphon_render::PatchLine> {
    let subject = selected
        .map(|message| message.subject.as_str())
        .unwrap_or_default();
    if !antiphon_render::is_patch(subject, body) {
        return Vec::new();
    }
    antiphon_render::classify_patch(body)
}

#[cfg(test)]
pub(super) fn app_with_messages(count: usize) -> App {
    let messages = (0..count)
        .map(|index| MessageSummary {
            id: format!("m{index}"),
            thread_id: String::new(),
            subject: String::new(),
            from: String::new(),
            to: String::new(),
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
        sync_progress: None,
        own_addresses: Vec::new(),
        preview: None,
        pager_body: String::new(),
        pager_patch: Vec::new(),
        pager_signature: Signature::none(),
        pager_invite: Vec::new(),
        pager_scroll: 0,
        pager_raw: Vec::new(),
        pager_html: false,
        preview_scroll: 0,
        keyring: Keyring::default(),
        reading_pane: ReadingPane::Below,
        sidebar: true,
        list_rows: antiphon_config::Ui::default().list_rows,
        sidebar_width: antiphon_config::Ui::default().sidebar_width,
        theme: &VESPERS,
        date_format: String::new(),
        notice: None,
        prompt: None,
        current_query: DEFAULT_QUERY.to_string(),
        pending_ops: Vec::new(),
        pending_template: None,
        pending_patches: None,
        pending_sign: None,
        pending_encrypt: None,
        pending_one_click: None,
        pending_unsubscribe: None,
        frame_stats: FrameStats::default(),
        composer: Composer::Embedded,
        editor: None,
        editor_return: View::List,
        requery: false,
        quit: false,
    }
}

#[cfg(test)]
pub(super) fn app_with_accounts(names: &[&str]) -> App {
    app_with_folders(
        &names
            .iter()
            .map(|name| (*name, &[][..]))
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
pub(super) fn app_with_folders(accounts: &[(&str, &[&str])]) -> App {
    let mut app = app_with_messages(1);
    app.accounts = accounts
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let entries: Vec<AccountEntry> = accounts
        .iter()
        .map(|(name, folders)| AccountEntry {
            name: (*name).to_string(),
            folders: folders
                .iter()
                .map(|folder| (*folder).to_string())
                .collect(),
        })
        .collect();
    app.sidebar_entries = sidebar::entries(&entries, &[]);
    app
}

fn own_addresses(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .flat_map(|entry| entry.account.identities.iter())
        .map(|identity| identity.address.to_lowercase())
        .collect()
}

impl App {
    /// Own mail shows who it went to, not who sent it, the
    /// way every sent folder is expected to read.
    pub fn is_own(&self, from: &str) -> bool {
        let lowered = from.to_lowercase();
        self.own_addresses
            .iter()
            .any(|address| lowered.contains(address.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn pager_scrolls_clamped_and_returns_to_the_list() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "one\ntwo\nthree\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
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
    fn pager_classification_follows_patch_detection() {
        use antiphon_render::PatchLine;

        let diff = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n";
        let mut app = app_with_messages(1);
        app.open_pager(
            "plain words\n".into(),
            Signature::none(),
            Vec::new(),
        );
        assert!(app.pager_patch.is_empty());
        app.open_pager(diff.into(), Signature::none(), Vec::new());
        assert_eq!(app.pager_patch[3], PatchLine::Removal);
        assert_eq!(app.pager_patch[4], PatchLine::Addition);
        app.messages[0].subject = "[PATCH] prose only".into();
        app.open_pager(
            "no diff here\n".into(),
            Signature::none(),
            Vec::new(),
        );
        assert_eq!(app.pager_patch, [PatchLine::Text]);
    }

    #[test]
    fn opening_the_pager_marks_the_message_read() {
        let mut app = app_with_messages(1);
        app.open_pager(String::new(), Signature::none(), Vec::new());
        assert!(!app.messages[0].unread);
        assert_eq!(app.pending_ops.len(), 1);
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
}
