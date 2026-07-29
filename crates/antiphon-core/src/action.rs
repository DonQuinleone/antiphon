use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    MoveDown,
    MoveUp,
    Top,
    Bottom,
    HalfPageDown,
    HalfPageUp,
    Open,
    Back,
    Quit,
    Search,
    Command,
    NextAccount,
    PreviousAccount,
    AccountTab(u8),
    AccountUnified,
    SidebarNext,
    SidebarPrevious,
    SidebarOpen,
    ToggleSidebar,
    CycleReadingPane,
    Sync,
    Reply,
    ReplyAll,
    ReplyList,
    Forward,
    Compose,
    ToggleRead,
    MarkAllRead,
    ToggleFlagged,
    ToggleHtml,
    OpenHtmlBrowser,
    PaneScrollDown,
    PaneScrollUp,
    Help,
    DeleteMessage,
    ToggleHeaders,
    OpenLink,
    Attachments,
    ThreadView,
    FoldToggle,
    FoldOpen,
    FoldClose,
    Archive,
    MoveTo,
    Settings,
    Send,
    EditBody,
    EditHeaders,
    AttachFile,
    RemoveAttachment,
    ToggleSign,
    ToggleEncrypt,
    SaveDraft,
    Schedule,
    NextTab,
    PrevTab,
    SettingsClose,
    ReorderDown,
    ReorderUp,
    AccountAdd,
    AccountEdit,
    DeleteAccount,
    SignIn,
    Revoke,
    SettingCycleNext,
    SettingCyclePrev,
    FolderHide,
    FolderUnsync,
    EditAlias,
    FocusNext,
    FocusPrev,
    ComposeSubmit,
    ComposeCancel,
    OpenEditor,
    PromptSubmit,
    PromptCancel,
}

const NAMES: &[(Action, &str)] = &[
    (Action::MoveDown, "move-down"),
    (Action::MoveUp, "move-up"),
    (Action::Top, "top"),
    (Action::Bottom, "bottom"),
    (Action::HalfPageDown, "half-page-down"),
    (Action::HalfPageUp, "half-page-up"),
    (Action::Open, "open"),
    (Action::Back, "back"),
    (Action::Quit, "quit"),
    (Action::Search, "search"),
    (Action::Command, "command"),
    (Action::NextAccount, "next-account"),
    (Action::PreviousAccount, "previous-account"),
    (Action::AccountTab(1), "account-1"),
    (Action::AccountTab(2), "account-2"),
    (Action::AccountTab(3), "account-3"),
    (Action::AccountTab(4), "account-4"),
    (Action::AccountTab(5), "account-5"),
    (Action::AccountTab(6), "account-6"),
    (Action::AccountTab(7), "account-7"),
    (Action::AccountTab(8), "account-8"),
    (Action::AccountTab(9), "account-9"),
    (Action::AccountUnified, "account-unified"),
    (Action::SidebarNext, "sidebar-next"),
    (Action::SidebarPrevious, "sidebar-previous"),
    (Action::SidebarOpen, "sidebar-open"),
    (Action::ToggleSidebar, "toggle-sidebar"),
    (Action::CycleReadingPane, "cycle-reading-pane"),
    (Action::Sync, "sync"),
    (Action::Reply, "reply"),
    (Action::ReplyAll, "reply-all"),
    (Action::ReplyList, "reply-list"),
    (Action::Forward, "forward"),
    (Action::Compose, "compose"),
    (Action::ToggleRead, "toggle-read"),
    (Action::MarkAllRead, "mark-all-read"),
    (Action::ToggleFlagged, "toggle-flagged"),
    (Action::ToggleHtml, "toggle-html"),
    (Action::OpenHtmlBrowser, "open-html-browser"),
    (Action::PaneScrollDown, "pane-down"),
    (Action::PaneScrollUp, "pane-up"),
    (Action::Help, "help"),
    (Action::DeleteMessage, "delete-message"),
    (Action::ToggleHeaders, "toggle-headers"),
    (Action::OpenLink, "open-link"),
    (Action::Attachments, "attachments"),
    (Action::ThreadView, "thread-view"),
    (Action::FoldToggle, "fold-toggle"),
    (Action::FoldOpen, "fold-open"),
    (Action::FoldClose, "fold-close"),
    (Action::Archive, "archive"),
    (Action::MoveTo, "move-to"),
    (Action::Settings, "settings"),
    (Action::Send, "send"),
    (Action::EditBody, "edit-body"),
    (Action::EditHeaders, "edit-headers"),
    (Action::AttachFile, "attach-file"),
    (Action::RemoveAttachment, "remove-attachment"),
    (Action::ToggleSign, "toggle-sign"),
    (Action::ToggleEncrypt, "toggle-encrypt"),
    (Action::SaveDraft, "save-draft"),
    (Action::Schedule, "schedule"),
    (Action::NextTab, "next-tab"),
    (Action::PrevTab, "prev-tab"),
    (Action::SettingsClose, "settings-close"),
    (Action::ReorderDown, "reorder-down"),
    (Action::ReorderUp, "reorder-up"),
    (Action::AccountAdd, "account-add"),
    (Action::AccountEdit, "account-edit"),
    (Action::DeleteAccount, "delete-account"),
    (Action::SignIn, "sign-in"),
    (Action::Revoke, "revoke"),
    (Action::SettingCycleNext, "setting-next"),
    (Action::SettingCyclePrev, "setting-prev"),
    (Action::FolderHide, "folder-hide"),
    (Action::FolderUnsync, "folder-unsync"),
    (Action::EditAlias, "edit-alias"),
    (Action::FocusNext, "focus-next"),
    (Action::FocusPrev, "focus-prev"),
    (Action::ComposeSubmit, "compose-submit"),
    (Action::ComposeCancel, "compose-cancel"),
    (Action::OpenEditor, "open-editor"),
    (Action::PromptSubmit, "prompt-submit"),
    (Action::PromptCancel, "prompt-cancel"),
];

impl Action {
    pub fn all() -> impl Iterator<Item = Self> {
        NAMES.iter().map(|(action, _)| *action)
    }

    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .expect("every action is named")
    }

    pub fn from_name(name: &str) -> Option<Self> {
        NAMES
            .iter()
            .find(|(_, candidate)| *candidate == name)
            .map(|(action, _)| *action)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.name())
    }
}

impl Action {
    /// Only motions repeat under a count prefix; anything
    /// with side effects runs once however many were typed.
    pub fn repeatable(self) -> bool {
        matches!(
            self,
            Action::MoveDown
                | Action::MoveUp
                | Action::HalfPageDown
                | Action::HalfPageUp
                | Action::PaneScrollDown
                | Action::PaneScrollUp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_round_trips_through_its_name() {
        for action in Action::all() {
            let name = action.name();
            assert_eq!(Action::from_name(name), Some(action));
            assert_eq!(action.to_string(), name);
        }
    }

    #[test]
    fn unknown_name_maps_to_nothing() {
        assert_eq!(Action::from_name("frobnicate"), None);
        assert_eq!(Action::from_name("HalfPageDown"), None);
    }
}
