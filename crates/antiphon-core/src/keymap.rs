use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::sequence::{Chord, KeySequence, SequenceError};

const COUNT_RADIX: u32 = 10;
const COUNT_CEILING: u32 = 9999;

/// The input surface a key is resolved against. A key can mean
/// different actions in different contexts (h toggles html in
/// the pager, edits headers in the review screen); Global is
/// the fallback checked after the active context, for keys that
/// mean the same thing everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    Global,
    List,
    Pager,
    Review,
    Settings,
    Compose,
    Prompt,
}

const DEFAULT_BINDINGS: &[(Context, Action, &str)] = &[
    (Context::Global, Action::MoveDown, "j"),
    (Context::Global, Action::MoveUp, "k"),
    (Context::Global, Action::Top, "gg"),
    (Context::Global, Action::Bottom, "G"),
    (Context::Global, Action::HalfPageDown, "ctrl-d"),
    (Context::Global, Action::HalfPageUp, "ctrl-u"),
    (Context::Global, Action::Open, "enter"),
    (Context::Global, Action::Back, "esc"),
    (Context::Global, Action::Quit, "q"),
    (Context::Global, Action::Search, "/"),
    (Context::Global, Action::Command, ":"),
    (Context::Global, Action::NextAccount, "gt"),
    (Context::Global, Action::PreviousAccount, "gT"),
    (Context::Global, Action::AccountTab(1), "g1"),
    (Context::Global, Action::AccountTab(2), "g2"),
    (Context::Global, Action::AccountTab(3), "g3"),
    (Context::Global, Action::AccountTab(4), "g4"),
    (Context::Global, Action::AccountTab(5), "g5"),
    (Context::Global, Action::AccountTab(6), "g6"),
    (Context::Global, Action::AccountTab(7), "g7"),
    (Context::Global, Action::AccountTab(8), "g8"),
    (Context::Global, Action::AccountTab(9), "g9"),
    (Context::Global, Action::AccountUnified, "gu"),
    (Context::Global, Action::SidebarNext, "ctrl-n"),
    (Context::Global, Action::SidebarPrevious, "ctrl-p"),
    (Context::Global, Action::SidebarOpen, "ctrl-o"),
    (Context::Global, Action::ToggleSidebar, "B"),
    (Context::Global, Action::CycleReadingPane, "p"),
    (Context::Global, Action::Sync, "s"),
    (Context::Global, Action::Reply, "r"),
    (Context::Global, Action::ReplyAll, "R"),
    (Context::Global, Action::ReplyList, "L"),
    (Context::Global, Action::Forward, "f"),
    (Context::Global, Action::Compose, "n"),
    (Context::Global, Action::ToggleRead, "m"),
    (Context::Global, Action::MarkAllRead, "M"),
    (Context::Global, Action::ToggleFlagged, "F"),
    (Context::Global, Action::DeleteMessage, "d"),
    (Context::Global, Action::ToggleHtml, "h"),
    (Context::Global, Action::OpenHtmlBrowser, "b"),
    (Context::Global, Action::PaneScrollDown, "J"),
    (Context::Global, Action::PaneScrollUp, "K"),
    (Context::Global, Action::Help, "?"),
    (Context::Global, Action::ToggleHeaders, "t"),
    (Context::Global, Action::OpenLink, "o"),
    (Context::Global, Action::Attachments, "v"),
    (Context::Global, Action::ThreadView, "T"),
    (Context::Global, Action::FoldToggle, "za"),
    (Context::Global, Action::FoldOpen, "zo"),
    (Context::Global, Action::FoldClose, "zc"),
    (Context::Global, Action::Archive, "a"),
    (Context::Global, Action::MoveTo, "c"),
    (Context::Global, Action::Settings, "<"),
    (Context::Review, Action::Send, "y"),
    (Context::Review, Action::EditBody, "e"),
    (Context::Review, Action::EditHeaders, "h"),
    (Context::Review, Action::AttachFile, "a"),
    (Context::Review, Action::RemoveAttachment, "d"),
    (Context::Review, Action::ToggleSign, "s"),
    (Context::Review, Action::ToggleEncrypt, "x"),
    (Context::Review, Action::SaveDraft, "q"),
    (Context::Review, Action::Schedule, "@"),
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
    singles: HashMap<(Context, Chord), Action>,
    pairs: HashMap<(Context, Chord, Chord), Action>,
    prefixes: HashSet<(Context, Chord)>,
    pending: Option<Chord>,
    count: u32,
    listing: Vec<(Action, String)>,
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
            entry.sequence = sequence;
            entry.text = text.clone();
            entry.user = true;
        }
        let listing: Vec<(Action, String)> = entries
            .iter()
            .map(|entry| (entry.action, entry.text.clone()))
            .collect();
        entries.sort_by_key(|entry| entry.user);

        let mut keymap = Self {
            singles: HashMap::new(),
            pairs: HashMap::new(),
            prefixes: HashSet::new(),
            pending: None,
            count: 0,
            listing,
        };
        for entry in entries {
            keymap.bind(entry.context, entry.action, entry.sequence);
        }
        Ok(keymap)
    }

    /// The pending count prefix, consumed on read; one when
    /// none was typed.
    pub fn take_count(&mut self) -> u32 {
        let count = self.count.max(1);
        self.count = 0;
        count
    }

    /// The effective bindings, defaults merged with the
    /// user's overrides, in the defaults' display order.
    pub fn bindings(&self) -> &[(Action, String)] {
        &self.listing
    }

    fn bind(
        &mut self,
        context: Context,
        action: Action,
        sequence: KeySequence,
    ) {
        match sequence {
            KeySequence::One(chord) => {
                self.singles.insert((context, chord), action);
            }
            KeySequence::Two(first, second) => {
                self.prefixes.insert((context, first));
                self.pairs.insert((context, first, second), action);
            }
        }
    }

    /// A single-chord binding for the context, falling back to
    /// Global for keys that mean the same thing everywhere.
    fn single(&self, context: Context, chord: Chord) -> Option<Action> {
        self.singles
            .get(&(context, chord))
            .or_else(|| self.singles.get(&(Context::Global, chord)))
            .copied()
    }

    fn is_prefix(&self, context: Context, chord: Chord) -> bool {
        self.prefixes.contains(&(context, chord))
            || self.prefixes.contains(&(Context::Global, chord))
    }

    fn pair(
        &self,
        context: Context,
        first: Chord,
        second: Chord,
    ) -> Option<Action> {
        self.pairs
            .get(&(context, first, second))
            .or_else(|| {
                self.pairs.get(&(Context::Global, first, second))
            })
            .copied()
    }

    /// A vim-style count prefix: digits accumulate before a
    /// binding and repeat it, e.g. 4j. A count only ever
    /// starts on a non-zero digit, so 0 stays bindable.
    pub fn feed(
        &mut self,
        context: Context,
        event: KeyEvent,
    ) -> Resolution {
        let chord = Chord::of(event);
        if self.pending.is_none()
            && let KeyCode::Char(digit @ '0'..='9') = chord.code
            && chord.modifiers.is_empty()
            && (self.count > 0 || digit != '0')
            && self.single(context, chord).is_none()
            && !self.is_prefix(context, chord)
        {
            let value = u32::from(digit as u8 - b'0');
            self.count =
                (self.count * COUNT_RADIX + value).min(COUNT_CEILING);
            return Resolution::Pending;
        }
        if let Some(prefix) = self.pending.take() {
            return match self.pair(context, prefix, chord) {
                Some(action) => Resolution::Match(action),
                None => Resolution::NoMatch,
            };
        }
        if self.is_prefix(context, chord) {
            self.pending = Some(chord);
            return Resolution::Pending;
        }
        match self.single(context, chord) {
            Some(action) => Resolution::Match(action),
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
    context: Context,
    action: Action,
    sequence: KeySequence,
    text: String,
    user: bool,
}

fn default_entries() -> Vec<Entry> {
    DEFAULT_BINDINGS
        .iter()
        .map(|(context, action, text)| Entry {
            context: *context,
            action: *action,
            sequence: text
                .parse()
                .expect("default key sequence parses"),
            text: (*text).to_string(),
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
    fn count_prefixes_accumulate_and_consume() {
        let mut keymap = Keymap::new(&overrides(&[])).unwrap();
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('4'))),
            Resolution::Pending
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('2'))),
            Resolution::Pending
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('j'))),
            Resolution::Match(Action::MoveDown)
        );
        assert_eq!(keymap.take_count(), 42);
        assert_eq!(keymap.take_count(), 1);
    }

    #[test]
    fn a_leading_zero_never_starts_a_count() {
        let mut keymap = Keymap::new(&overrides(&[])).unwrap();
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('0'))),
            Resolution::NoMatch
        );
        assert_eq!(keymap.take_count(), 1);
    }

    #[test]
    fn defaults_cover_every_action() {
        for action in Action::all() {
            assert!(
                DEFAULT_BINDINGS
                    .iter()
                    .any(|(_, bound, _)| *bound == action),
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
            (press(KeyCode::Char('p')), Action::CycleReadingPane),
            (shifted('R'), Action::ReplyAll),
            (press(KeyCode::Char('f')), Action::Forward),
            (press(KeyCode::Char('s')), Action::Sync),
            (press(KeyCode::Char('r')), Action::Reply),
            (shifted('L'), Action::ReplyList),
            (press(KeyCode::Char('n')), Action::Compose),
            (press(KeyCode::Char('t')), Action::ToggleHeaders),
            (press(KeyCode::Char('o')), Action::OpenLink),
            (press(KeyCode::Char('v')), Action::Attachments),
            (shifted('T'), Action::ThreadView),
            (press(KeyCode::Char('a')), Action::Archive),
            (press(KeyCode::Char('c')), Action::MoveTo),
            (press(KeyCode::Char('<')), Action::Settings),
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
                keymap.feed(Context::List, event),
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
            (press(KeyCode::Char('1')), Action::AccountTab(1)),
            (press(KeyCode::Char('9')), Action::AccountTab(9)),
            (press(KeyCode::Char('u')), Action::AccountUnified),
        ];
        for (second, action) in cases {
            let mut keymap = Keymap::default();
            assert_eq!(
                keymap.feed(Context::List, press(KeyCode::Char('g'))),
                Resolution::Pending,
            );
            assert_eq!(
                keymap.feed(Context::List, second),
                Resolution::Match(action),
                "event {second:?}",
            );
        }
    }

    #[test]
    fn pending_then_wrong_key_resets() {
        let mut keymap = Keymap::default();
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('g'))),
            Resolution::Pending,
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('x'))),
            Resolution::NoMatch,
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('j'))),
            Resolution::Match(Action::MoveDown),
        );
    }

    #[test]
    fn override_wins_over_default() {
        let mut keymap = Keymap::new(&overrides(&[("quit", "x")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('x'))),
            Resolution::Match(Action::Quit),
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('q'))),
            Resolution::NoMatch,
        );
    }

    #[test]
    fn override_may_rebind_to_a_two_key_sequence() {
        let mut keymap = Keymap::new(&overrides(&[("sync", "zs")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('z'))),
            Resolution::Pending,
        );
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('s'))),
            Resolution::Match(Action::Sync),
        );
    }

    #[test]
    fn override_steals_a_default_key() {
        let mut keymap = Keymap::new(&overrides(&[("sync", "r")]))
            .expect("override should build");
        assert_eq!(
            keymap.feed(Context::List, press(KeyCode::Char('r'))),
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

    #[test]
    fn default_bindings_bind_each_key_once() {
        let mut seen = std::collections::HashSet::new();
        for (context, _, key) in DEFAULT_BINDINGS {
            assert!(
                seen.insert((context, *key)),
                "{key:?} is bound more than once in {context:?}"
            );
        }
    }

    #[test]
    fn a_context_binding_overrides_the_global_one() {
        let mut keymap = Keymap::new(&overrides(&[])).unwrap();
        assert_eq!(
            keymap.feed(Context::Pager, press(KeyCode::Char('h'))),
            Resolution::Match(Action::ToggleHtml)
        );
        assert_eq!(
            keymap.feed(Context::Review, press(KeyCode::Char('h'))),
            Resolution::Match(Action::EditHeaders)
        );
        assert_eq!(
            keymap.feed(Context::Review, press(KeyCode::Char('j'))),
            Resolution::Match(Action::MoveDown),
            "a global key still resolves via the fallback"
        );
    }
}
