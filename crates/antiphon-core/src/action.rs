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
    PaneScrollDown,
    PaneScrollUp,
    Help,
    DeleteMessage,
    ToggleHeaders,
    OpenLink,
    Attachments,
    ThreadView,
    Archive,
    MoveTo,
    Settings,
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
    (Action::PaneScrollDown, "pane-down"),
    (Action::PaneScrollUp, "pane-up"),
    (Action::Help, "help"),
    (Action::DeleteMessage, "delete-message"),
    (Action::ToggleHeaders, "toggle-headers"),
    (Action::OpenLink, "open-link"),
    (Action::Attachments, "attachments"),
    (Action::ThreadView, "thread-view"),
    (Action::Archive, "archive"),
    (Action::MoveTo, "move-to"),
    (Action::Settings, "settings"),
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
