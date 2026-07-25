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
    SidebarNext,
    SidebarPrevious,
    SidebarOpen,
    ToggleSidebar,
    CycleReadingPane,
    Sync,
    Reply,
    ReplyList,
    Compose,
    MarkRead,
    MarkUnread,
    ToggleFlagged,
    ToggleHtml,
    PaneScrollDown,
    PaneScrollUp,
    DeleteMessage,
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
    (Action::SidebarNext, "sidebar-next"),
    (Action::SidebarPrevious, "sidebar-previous"),
    (Action::SidebarOpen, "sidebar-open"),
    (Action::ToggleSidebar, "toggle-sidebar"),
    (Action::CycleReadingPane, "cycle-reading-pane"),
    (Action::Sync, "sync"),
    (Action::Reply, "reply"),
    (Action::ReplyList, "reply-list"),
    (Action::Compose, "compose"),
    (Action::MarkRead, "mark-read"),
    (Action::MarkUnread, "mark-unread"),
    (Action::ToggleFlagged, "toggle-flagged"),
    (Action::ToggleHtml, "toggle-html"),
    (Action::PaneScrollDown, "pane-down"),
    (Action::PaneScrollUp, "pane-up"),
    (Action::DeleteMessage, "delete-message"),
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
