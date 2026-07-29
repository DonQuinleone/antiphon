use antiphon_config::{Loaded, ReadingPane};
use antiphon_core::Action;

pub(super) use super::mailops::{OpIntent, account_of, folder_of};

use super::app::App;
use super::commands::PromptKind;
use super::scope::{self, ViewScope};
use super::sidebar::{self, SidebarEntry};

const HALF_PAGE_ROWS: usize = 10;
const OPEN_MESSAGE_FIRST: &str = "open a message first";

pub fn account_names(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .map(|entry| entry.account.account.name.clone())
        .collect()
}

const THREAD_LABEL: &str = "thread";

enum Fold {
    Toggle,
    Open,
    Close,
}

/// Walks a visibility step `rows` times, clamping at whichever
/// end runs out of visible nodes.
fn step(
    rows: usize,
    from: usize,
    next: impl Fn(usize) -> usize,
) -> usize {
    let mut position = from;
    for _ in 0..rows {
        position = next(position);
    }
    position
}

impl App {
    pub(super) fn apply_in_list(&mut self, action: Action) {
        match action {
            Action::MoveDown => self.select_forward(1),
            Action::MoveUp => self.select_back(1),
            Action::PaneScrollDown => {
                self.preview_scroll =
                    self.preview_scroll.saturating_add(1)
            }
            Action::PaneScrollUp => {
                self.preview_scroll =
                    self.preview_scroll.saturating_sub(1)
            }
            Action::HalfPageDown => self.select_forward(HALF_PAGE_ROWS),
            Action::HalfPageUp => self.select_back(HALF_PAGE_ROWS),
            Action::Top => self.selected = 0,
            Action::Bottom => self.selected = self.bottom_index(),
            Action::ToggleSidebar => self.sidebar = !self.sidebar,
            Action::ToggleHeaders => {
                self.headers_all = !self.headers_all
            }
            Action::OpenLink => self.open_preview_link_picker(),
            Action::Attachments => self.toggle_preview_drawer(),
            Action::NextAccount => self.shift_scope(scope::next_scope),
            Action::PreviousAccount => {
                self.shift_scope(scope::previous_scope)
            }
            Action::AccountTab(tab) => self.open_account_tab(tab),
            Action::AccountUnified => self.open_unified_tab(),
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
            Action::ToggleRead => self.toggle_read(),
            Action::ToggleFlagged => self.toggle_flagged(),
            Action::DeleteMessage => self.delete_selected(),
            Action::Archive => self.archive_selected(),
            Action::MoveTo => self.open_folder_picker(),
            Action::ThreadView => self.open_thread(),
            Action::FoldToggle => self.fold_selected(Fold::Toggle),
            Action::FoldOpen => self.fold_selected(Fold::Open),
            Action::FoldClose => self.fold_selected(Fold::Close),
            Action::Back => self.close_thread(),
            Action::Quit => self.quit = true,
            _ => self.not_built_notice(),
        }
    }

    /// The list stays flat; T pivots it onto the selected
    /// message's whole thread, and back restores the listing
    /// it came from.
    fn open_thread(&mut self) {
        let Some(message) = self.selected_message() else {
            return;
        };
        let thread = message.thread_id.clone();
        if thread.is_empty() {
            self.notice = Some("no thread for this message".into());
            return;
        }
        if self.thread_return.is_none() {
            self.thread_return = Some((
                self.current_query.clone(),
                self.active_search.clone(),
            ));
        }
        self.current_query = format!("thread:{thread}");
        self.active_search = Some(THREAD_LABEL.to_string());
        self.requery = true;
    }

    fn close_thread(&mut self) {
        let Some((query, search)) = self.thread_return.take() else {
            return;
        };
        self.thread_tree = None;
        self.current_query = query;
        self.active_search = search;
        self.requery = true;
    }

    /// Folds the selected subtree; a bare leaf or a list with no
    /// tree says so rather than silently doing nothing.
    fn fold_selected(&mut self, fold: Fold) {
        let position = self.selected;
        let Some(tree) = self.thread_tree.as_mut() else {
            self.notice = Some("open a thread first".into());
            return;
        };
        let changed = match fold {
            Fold::Toggle => tree.toggle(position),
            Fold::Open => tree.set_collapsed(position, false),
            Fold::Close => tree.set_collapsed(position, true),
        };
        if !changed {
            self.notice = Some("no replies to fold here".into());
        }
    }

    fn bottom_index(&self) -> usize {
        match &self.thread_tree {
            Some(tree) => tree.last_visible(),
            None => self.last_index(),
        }
    }

    fn shift_scope(
        &mut self,
        step: fn(&ViewScope, &[String]) -> ViewScope,
    ) {
        self.switch_scope(step(&self.scope, &self.accounts));
    }

    pub(super) fn sidebar_open(&mut self) {
        self.thread_return = None;
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
            SidebarEntry::Folder {
                account,
                name,
                query,
                ..
            } => {
                let label = self
                    .alias_for(&account, &name)
                    .unwrap_or(&name)
                    .to_string();
                self.scope = ViewScope::Account(account);
                self.current_query = query;
                self.active_search = Some(label);
            }
            SidebarEntry::Saved { name, query } => {
                self.current_query = query;
                self.active_search = Some(name);
            }
        }
        self.requery = true;
        self.sync_tab_sidebar();
    }

    fn select_forward(&mut self, rows: usize) {
        self.selected = match &self.thread_tree {
            Some(tree) => step(rows, self.selected, |from| {
                tree.next_visible(from)
            }),
            None => (self.selected + rows).min(self.last_index()),
        };
    }

    fn select_back(&mut self, rows: usize) {
        self.selected = match &self.thread_tree {
            Some(tree) => step(rows, self.selected, |from| {
                tree.prev_visible(from)
            }),
            None => self.selected.saturating_sub(rows),
        };
    }

    pub(super) fn last_index(&self) -> usize {
        self.messages.len().saturating_sub(1)
    }

    fn cycle_reading_pane(&mut self) {
        self.reading_pane = match self.reading_pane {
            ReadingPane::Below => ReadingPane::Right,
            ReadingPane::Right => ReadingPane::Off,
            ReadingPane::Off => ReadingPane::Below,
        };
    }

    /// The reading pane shows a preview; without one there is
    /// nothing to open links or attachments over, so the list
    /// asks the reader to open the message in the pager first.
    fn reading_pane_active(&self) -> bool {
        self.reading_pane != ReadingPane::Off && self.preview.is_some()
    }

    fn open_preview_link_picker(&mut self) {
        if !self.reading_pane_active() {
            self.notice = Some(OPEN_MESSAGE_FIRST.to_string());
            return;
        }
        self.load_preview_extras();
        self.open_link_picker();
    }

    fn toggle_preview_drawer(&mut self) {
        if !self.reading_pane_active() {
            self.notice = Some(OPEN_MESSAGE_FIRST.to_string());
            return;
        }
        if self.drawer_open {
            self.drawer_open = false;
            return;
        }
        self.drawer_selected = 0;
        self.load_preview_extras();
        self.open_drawer();
    }
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
