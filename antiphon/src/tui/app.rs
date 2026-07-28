use antiphon_config::{
    AccountsBar, Composer, Dirs, Loaded, ReadingPane,
};
use antiphon_core::Action;
use antiphon_pgp::{Keyring, Signature};
use antiphon_render::{
    MailtoUnsubscribe, MessageAttachment, MessageHeader, RenderedBody,
};
use antiphon_store::MessageSummary;
use antiphon_store::contacts::Contact;
use antiphon_ui::Theme;

use super::actions::{OpIntent, account_names};
use super::app_sidebar::initial_scope;
use super::commands::{
    ExportCommand, FrameStats, PatchCommand, Prompt,
};
use super::compose::ComposeState;
use super::editor::EditorPane;
use super::link_picker::LinkPicker;
use super::prefs::{
    archive_folders, folder_aliases, own_addresses, trash_folders,
};
use super::scope::{self, ViewScope};
use super::sidebar::{self, AccountEntry, SidebarEntry};
use antiphon_store::ScopeError;

pub const DEFAULT_QUERY: &str = "*";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    List,
    Pager,
    Compose,
    Editor,
    Review,
    Settings,
}

/// Where the next key event goes; resolved before the keymap
/// so an open editor swallows everything raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    Prompt,
    Compose,
    Editor,
    Review,
    Settings,
    Keymap,
}

pub struct App {
    pub accounts: Vec<String>,
    pub scope: ViewScope,
    pub account_entries: Vec<AccountEntry>,
    pub saved_searches: Vec<antiphon_config::SavedSearch>,
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
    pub pager_headers: Vec<MessageHeader>,
    pub pager_headers_all: Vec<MessageHeader>,
    pub pager_rendered: RenderedBody,
    pub pager_attachments: Vec<MessageAttachment>,
    pub link_picker: Option<LinkPicker>,
    pub folder_picker: Option<super::folder_picker::FolderPicker>,
    pub account_form: Option<super::account_form::AccountFormState>,
    pub folder_alias_edit: Option<super::folder_alias::AliasEdit>,
    pub drawer_open: bool,
    pub drawer_selected: usize,
    pub header_names: Vec<String>,
    pub headers_all: bool,
    pub preview_scroll: u16,
    pub preview_html: bool,
    pub help: bool,
    pub help_scroll: u16,
    pub key_bindings: Vec<(String, String)>,
    pub keyring: Keyring,
    pub own_addresses: Vec<String>,
    pub archive_folders: Vec<(String, String)>,
    pub trash_folders: Vec<(String, String)>,
    pub folder_aliases: Vec<(String, String, String)>,
    pub contacts: Vec<Contact>,
    pub preview: Option<super::preview::Preview>,
    pub reading_pane: ReadingPane,
    pub accounts_bar: AccountsBar,
    pub sidebar: bool,
    pub list_rows: u16,
    pub sidebar_width: u16,
    pub theme: &'static Theme,
    pub(super) config_path: std::path::PathBuf,
    pub(super) dirs: Dirs,
    pub sync_interval_minutes: u32,
    pub sync_idle: bool,
    pub settings: Option<super::settings::SettingsState>,
    pub oauth_flow: Option<super::oauthflow::OauthFlow>,
    pub date_format: String,
    pub notice: Option<String>,
    pub prompt: Option<Prompt>,
    pub current_query: String,
    pub pending_ops: Vec<OpIntent>,
    pub sync_progress: Option<antiphon_sync::SyncProgress>,
    pub pending_template: Option<String>,
    pub pending_resume: Option<std::path::PathBuf>,
    pub pending_patches: Option<PatchCommand>,
    pub pending_export: Option<ExportCommand>,
    pub export_recipients: Vec<String>,
    pub(super) pending_sign: Option<bool>,
    pub(super) pending_encrypt: Option<bool>,
    pub(super) pending_rsvp: Option<antiphon_render::Rsvp>,
    pub(super) pending_one_click: Option<String>,
    pub(super) pending_unsub_post: Option<String>,
    pub(super) pending_unsubscribe: Option<(String, MailtoUnsubscribe)>,
    pub frame_stats: FrameStats,
    pub composer: Composer,
    pub compose: Option<ComposeState>,
    pub editor: Option<EditorPane>,
    pub(super) editor_return: View,
    pub(super) thread_return: Option<(String, Option<String>)>,
    pub(super) requery: bool,
    pub read_only: bool,
    pub quit: bool,
}

impl App {
    pub fn new(
        loaded: &Loaded,
        folders: &[AccountEntry],
        messages: Vec<MessageSummary>,
        total_messages: u32,
        keyring: Keyring,
        config_path: std::path::PathBuf,
        dirs: &Dirs,
    ) -> App {
        let accounts = account_names(loaded);
        let own_addresses = own_addresses(loaded);
        let scope = initial_scope(loaded, &accounts);
        let sidebar_entries = match loaded.config.ui.accounts_bar {
            AccountsBar::Sidebar => {
                sidebar::entries(folders, &loaded.config.saved_searches)
            }
            AccountsBar::Tabs => sidebar::tab_entries(
                folders,
                &loaded.config.saved_searches,
                match &scope {
                    ViewScope::Unified => None,
                    ViewScope::Account(account) => {
                        Some(account.as_str())
                    }
                },
            ),
        };
        let sidebar_selected =
            sidebar::default_selection(&sidebar_entries);
        let theme = Theme::by_name(&loaded.config.ui.theme)
            .unwrap_or(Theme::vespers());
        App {
            accounts,
            scope,
            account_entries: folders.to_vec(),
            saved_searches: loaded.config.saved_searches.clone(),
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
            pager_headers: Vec::new(),
            pager_headers_all: Vec::new(),
            pager_rendered: RenderedBody::default(),
            pager_attachments: Vec::new(),
            link_picker: None,
            folder_picker: None,
            account_form: None,
            folder_alias_edit: None,
            drawer_open: false,
            drawer_selected: 0,
            header_names: loaded.config.ui.headers.clone(),
            headers_all: false,
            preview_scroll: 0,
            preview_html: false,
            help: false,
            help_scroll: 0,
            key_bindings: Vec::new(),
            keyring,
            own_addresses,
            archive_folders: archive_folders(loaded),
            trash_folders: trash_folders(loaded),
            folder_aliases: folder_aliases(loaded),
            contacts: Vec::new(),
            preview: None,
            reading_pane: loaded.config.ui.reading_pane,
            accounts_bar: loaded.config.ui.accounts_bar,
            sidebar: true,
            list_rows: loaded.config.ui.list_rows,
            sidebar_width: loaded.config.ui.sidebar_width,
            theme,
            config_path,
            dirs: dirs.clone(),
            sync_interval_minutes: loaded.config.sync.interval_minutes,
            sync_idle: loaded.config.sync.idle,
            settings: None,
            oauth_flow: None,
            date_format: loaded.config.ui.date_format.clone(),
            notice: None,
            prompt: None,
            current_query: DEFAULT_QUERY.to_string(),
            pending_ops: Vec::new(),
            sync_progress: None,
            pending_template: None,
            pending_resume: None,
            pending_patches: None,
            pending_export: None,
            export_recipients: loaded.config.export.recipients.clone(),
            pending_sign: None,
            pending_encrypt: None,
            pending_rsvp: None,
            pending_one_click: None,
            pending_unsub_post: None,
            pending_unsubscribe: None,
            frame_stats: FrameStats::default(),
            composer: loaded.config.ui.composer,
            compose: None,
            editor: None,
            editor_return: View::List,
            thread_return: None,
            requery: false,
            read_only: false,
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

    /// The account a fresh compose starts from: the account in
    /// view when the scope is pinned to one, else the primary
    /// account (the first configured). The From field can still
    /// cycle to any identity afterwards.
    pub fn compose_account(&self) -> String {
        match &self.scope {
            ViewScope::Account(account) => account.clone(),
            ViewScope::Unified => {
                self.accounts.first().cloned().unwrap_or_default()
            }
        }
    }

    pub fn take_requery(&mut self) -> bool {
        std::mem::take(&mut self.requery)
    }

    pub fn key_route(&self) -> KeyRoute {
        if self.prompt.is_some() {
            return KeyRoute::Prompt;
        }
        match self.view {
            View::Compose => KeyRoute::Compose,
            View::Editor => KeyRoute::Editor,
            View::Review => KeyRoute::Review,
            View::Settings => KeyRoute::Settings,
            _ => KeyRoute::Keymap,
        }
    }

    pub fn start_compose(&mut self, mut state: ComposeState) {
        state.contacts = self.contacts.clone();
        self.compose = Some(state);
        self.view = View::Compose;
    }

    pub fn abort_compose(&mut self, notice: &str) {
        self.discard_editor();
        self.compose = None;
        self.view = View::List;
        self.notice = Some(notice.to_string());
    }

    /// Kills any editor still attached to the compose being
    /// torn down; a surviving pane would silently re-attach
    /// to the next compose.
    pub fn discard_editor(&mut self) {
        let Some(mut pane) = self.editor.take() else {
            return;
        };
        pane.session.kill();
        let _ = std::fs::remove_file(&pane.path);
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
            View::Compose
            | View::Editor
            | View::Review
            | View::Settings => {}
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

    pub(super) fn not_built_notice(&mut self) {
        self.notice = Some("not built yet".to_string());
    }
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
#[path = "app_tests.rs"]
mod tests;
