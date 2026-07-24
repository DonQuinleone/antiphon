use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crossterm::event::KeyEvent;

use crate::action::Action;
use crate::sequence::{Chord, KeySequence, SequenceError};

const DEFAULT_BINDINGS: &[(Action, &str)] = &[
    (Action::MoveDown, "j"),
    (Action::MoveUp, "k"),
    (Action::Top, "gg"),
    (Action::Bottom, "G"),
    (Action::HalfPageDown, "ctrl-d"),
    (Action::HalfPageUp, "ctrl-u"),
    (Action::Open, "enter"),
    (Action::Back, "esc"),
    (Action::Quit, "q"),
    (Action::Search, "/"),
    (Action::Command, ":"),
    (Action::NextAccount, "gt"),
    (Action::PreviousAccount, "gT"),
    (Action::SidebarNext, "ctrl-n"),
    (Action::SidebarPrevious, "ctrl-p"),
    (Action::SidebarOpen, "ctrl-o"),
    (Action::ToggleSidebar, "B"),
    (Action::CycleReadingPane, "R"),
    (Action::Sync, ",s"),
    (Action::Reply, "r"),
    (Action::ReplyList, "L"),
    (Action::Compose, "n"),
    (Action::MarkRead, "m"),
    (Action::MarkUnread, "M"),
    (Action::ToggleFlagged, "F"),
    (Action::DeleteMessage, "d"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Pending,
    Match(Action),
    NoMatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapError {
    UnknownAction {
        action: String,
    },
    BadSequence {
        action: String,
        sequence: String,
        error: SequenceError,
    },
}

impl fmt::Display for KeymapError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAction { action } => {
                write!(out, "unknown action `{action}` in [keys]")
            }
            Self::BadSequence {
                action,
                sequence,
                error,
            } => {
                write!(
                    out,
                    "invalid key sequence `{sequence}` for \
                     `{action}`: {error}",
                )
            }
        }
    }
}

impl std::error::Error for KeymapError {}

#[derive(Debug)]
pub struct Keymap {
    singles: HashMap<Chord, Action>,
    pairs: HashMap<(Chord, Chord), Action>,
    prefixes: HashSet<Chord>,
    pending: Option<Chord>,
}

impl Keymap {
    pub fn new(
        overrides: &BTreeMap<String, String>,
    ) -> Result<Self, KeymapError> {
        let mut entries = default_entries();
        for (name, text) in overrides {
            let action = Action::from_name(name).ok_or_else(|| {
                KeymapError::UnknownAction {
                    action: name.clone(),
                }
            })?;
            let sequence = text.parse().map_err(|error| {
                KeymapError::BadSequence {
                    action: name.clone(),
                    sequence: text.clone(),
                    error,
                }
            })?;
            let entry = entries
                .iter_mut()
                .find(|entry| entry.action == action)
                .expect("defaults cover every action");
            *entry = Entry {
                action,
                sequence,
                user: true,
            };
        }
        entries.sort_by_key(|entry| entry.user);

        let mut keymap = Self {
            singles: HashMap::new(),
            pairs: HashMap::new(),
            prefixes: HashSet::new(),
            pending: None,
        };
        for entry in entries {
            keymap.bind(entry.action, entry.sequence);
        }
        Ok(keymap)
    }

    fn bind(&mut self, action: Action, sequence: KeySequence) {
        match sequence {
            KeySequence::One(chord) => {
                self.singles.insert(chord, action);
            }
            KeySequence::Two(first, second) => {
                self.prefixes.insert(first);
                self.pairs.insert((first, second), action);
            }
        }
    }

    pub fn feed(&mut self, event: KeyEvent) -> Resolution {
        let chord = Chord::of(event);
        if let Some(prefix) = self.pending.take() {
            return match self.pairs.get(&(prefix, chord)) {
                Some(action) => Resolution::Match(*action),
                None => Resolution::NoMatch,
            };
        }
        if self.prefixes.contains(&chord) {
            self.pending = Some(chord);
            return Resolution::Pending;
        }
        match self.singles.get(&chord) {
            Some(action) => Resolution::Match(*action),
            None => Resolution::NoMatch,
        }
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(&BTreeMap::new()).expect("default bindings are valid")
    }
}

struct Entry {
    action: Action,
    sequence: KeySequence,
    user: bool,
}

fn default_entries() -> Vec<Entry> {
    DEFAULT_BINDINGS
        .iter()
        .map(|(action, text)| Entry {
            action: *action,
            sequence: text
                .parse()
                .expect("default key sequence parses"),
            user: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shifted(letter: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT)
    }

    fn overrides(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, text)| {
                ((*name).to_owned(), (*text).to_owned())
            })
            .collect()
    }

    #[test]
    fn defaults_cover_every_action() {
        for action in Action::all() {
            assert!(
                DEFAULT_BINDINGS
                    .iter()
                    .any(|(bound, _)| *bound == action),
                "no default binding for {action}",
            );
        }
    }

    #[test]
    fn default_single_key_lookups() {
        let cases = [
            (press(KeyCode::Char('j')), Action::MoveDown),
            (press(KeyCode::Char('k')), Action::MoveUp),
            (shifted('G'), Action::Bottom),
            (
                KeyEvent::new(
                    KeyCode::Char('d'),
                    KeyModifiers::CONTROL,
                ),
                Action::HalfPageDown,
            ),
            (
                KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ),
                Action::HalfPageUp,
            ),
            (press(KeyCode::Enter), Action::Open),
            (press(KeyCode::Esc), Action::Back),
            (press(KeyCode::Char('q')), Action::Quit),
            (press(KeyCode::Char('/')), Action::Search),
            (press(KeyCode::Char(':')), Action::Command),
            (shifted('B'), Action::ToggleSidebar),
            (shifted('R'), Action::CycleReadingPane),
            (press(KeyCode::Char('r')), Action::Reply),
            (shifted('L'), Action::ReplyList),
            (press(KeyCode::Char('n')), Action::Compose),
            (
                KeyEvent::new(
                    KeyCode::Char('n'),
                    KeyModifiers::CONTROL,
                ),
                Action::SidebarNext,
            ),
            (
                KeyEvent::new(
                    KeyCode::Char('p'),
                    KeyModifiers::CONTROL,
                ),
                Action::SidebarPrevious,
            ),
            (
                KeyEvent::new(
                    KeyCode::Char('o'),
                    KeyModifiers::CONTROL,
                ),
                Action::SidebarOpen,
            ),
        ];
        for (event, action) in cases {
            let mut keymap = Keymap::default();
            assert_eq!(
                keymap.feed(event),
                Resolution::Match(action),
                "event {event:?}",
            );
        }
    }

    #[test]
    fn default_two_key_lookups() {
        let cases = [
            (press(KeyCode::Char('g')), Action::Top),
            (press(KeyCode::Char('t')), Action::NextAccount),
            (shifted('T'), Action::PreviousAccount),
        ];
        for (second, action) in cases {
            let mut keymap = Keymap::default();
            assert_eq!(
                keymap.feed(press(KeyCode::Char('g'))),
                Resolution::Pending,
            );
            assert_eq!(
                keymap.feed(second),
                Resolution::Match(action),
                "event {second:?}",
            );
        }

        let mut keymap = Keymap::default();
        assert_eq!(
            keymap.feed(press(KeyCode::Char(','))),
            Resolution::Pending,
        );
        assert_eq!(
            keymap.feed(press(KeyCode::Char('s'))),
            Resolution::Match(Action::Sync),
        );
    }

    #[test]
    fn pending_then_wrong_key_resets() {
        let mut keymap = Keymap::default();
        assert_eq!(
            keymap.feed(press(KeyCode::Char('g'))),
            Resolution::Pending,
        );
        assert_eq!(
            keymap.feed(press(KeyCode::Char('x'))),
            Resolution::NoMatch,
        );
        assert_eq!(
            keymap.feed(press(KeyCode::Char('j'))),
            Resolution::Match(Action::MoveDown),
        );
    }

    #[test]
    fn override_wins_over_default() {
        let mut keymap = Keymap::new(&overrides(&[("quit", "x")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(press(KeyCode::Char('x'))),
            Resolution::Match(Action::Quit),
        );
        assert_eq!(
            keymap.feed(press(KeyCode::Char('q'))),
            Resolution::NoMatch,
        );
    }

    #[test]
    fn override_may_rebind_to_a_two_key_sequence() {
        let mut keymap = Keymap::new(&overrides(&[("sync", "zs")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(press(KeyCode::Char('z'))),
            Resolution::Pending,
        );
        assert_eq!(
            keymap.feed(press(KeyCode::Char('s'))),
            Resolution::Match(Action::Sync),
        );
    }

    #[test]
    fn override_steals_a_default_key() {
        let mut keymap = Keymap::new(&overrides(&[("sync", "r")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(press(KeyCode::Char('r'))),
            Resolution::Match(Action::Sync),
        );
    }

    #[test]
    fn unknown_action_is_reported() {
        let error = Keymap::new(&overrides(&[("frobnicate", "x")]))
            .expect_err("unknown action should fail");
        assert_eq!(
            error,
            KeymapError::UnknownAction {
                action: "frobnicate".to_owned(),
            },
        );
    }

    #[test]
    fn bad_sequence_carries_the_offending_entry() {
        let error = Keymap::new(&overrides(&[("quit", "ctlr-q")]))
            .expect_err("bad sequence should fail");
        assert_eq!(
            error,
            KeymapError::BadSequence {
                action: "quit".to_owned(),
                sequence: "ctlr-q".to_owned(),
                error: SequenceError {
                    token: "ctlr".to_owned(),
                },
            },
        );
        let message = error.to_string();
        assert!(message.contains("quit"));
        assert!(message.contains("ctlr"));
    }
}
