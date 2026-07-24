use std::fmt;
use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl Chord {
    fn plain(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub fn of(event: KeyEvent) -> Self {
        let modifiers = match event.code {
            KeyCode::Char(_) => {
                event.modifiers.difference(KeyModifiers::SHIFT)
            }
            _ => event.modifiers,
        };
        Self {
            code: event.code,
            modifiers,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySequence {
    One(Chord),
    Two(Chord, Chord),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceError {
    pub token: String,
}

impl SequenceError {
    fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
        }
    }
}

impl fmt::Display for SequenceError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "unrecognised key token `{}`", self.token)
    }
}

impl std::error::Error for SequenceError {}

const NAMED_KEYS: &[(&str, KeyCode)] = &[
    ("enter", KeyCode::Enter),
    ("esc", KeyCode::Esc),
    ("tab", KeyCode::Tab),
    ("space", KeyCode::Char(' ')),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
];

const MODIFIERS: &[(&str, KeyModifiers)] = &[
    ("ctrl", KeyModifiers::CONTROL),
    ("alt", KeyModifiers::ALT),
    ("shift", KeyModifiers::SHIFT),
];

fn single_char(token: &str) -> Option<char> {
    let mut chars = token.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(first)
}

fn base_key(token: &str) -> Option<KeyCode> {
    if let Some(letter) = single_char(token) {
        return Some(KeyCode::Char(letter));
    }
    NAMED_KEYS
        .iter()
        .find(|(name, _)| *name == token)
        .map(|(_, code)| *code)
}

fn modifier(token: &str) -> Option<KeyModifiers> {
    MODIFIERS
        .iter()
        .find(|(name, _)| *name == token)
        .map(|(_, flag)| *flag)
}

fn parse_modified(text: &str) -> Result<Chord, SequenceError> {
    let (prefix, key) =
        text.rsplit_once('-').expect("caller checked for a dash");
    let code = base_key(key).ok_or_else(|| SequenceError::new(key))?;
    let mut modifiers = KeyModifiers::NONE;
    for name in prefix.split('-') {
        let flag =
            modifier(name).ok_or_else(|| SequenceError::new(name))?;
        modifiers |= flag;
    }
    Ok(Chord { code, modifiers })
}

impl FromStr for KeySequence {
    type Err = SequenceError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(code) = base_key(text) {
            return Ok(Self::One(Chord::plain(code)));
        }
        if text.contains('-') {
            return Ok(Self::One(parse_modified(text)?));
        }
        let letters: Vec<char> = text.chars().collect();
        if let [first, second] = letters[..] {
            return Ok(Self::Two(
                Chord::plain(KeyCode::Char(first)),
                Chord::plain(KeyCode::Char(second)),
            ));
        }
        Err(SequenceError::new(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(code: KeyCode, modifiers: KeyModifiers) -> KeySequence {
        KeySequence::One(Chord { code, modifiers })
    }

    fn two(first: char, second: char) -> KeySequence {
        KeySequence::Two(
            Chord::plain(KeyCode::Char(first)),
            Chord::plain(KeyCode::Char(second)),
        )
    }

    #[test]
    fn parses_valid_sequences() {
        let none = KeyModifiers::NONE;
        let cases = [
            ("j", one(KeyCode::Char('j'), none)),
            ("/", one(KeyCode::Char('/'), none)),
            ("G", one(KeyCode::Char('G'), none)),
            ("-", one(KeyCode::Char('-'), none)),
            ("ctrl-d", one(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            ("alt-v", one(KeyCode::Char('v'), KeyModifiers::ALT)),
            (
                "ctrl-alt-x",
                one(
                    KeyCode::Char('x'),
                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                ),
            ),
            ("shift-tab", one(KeyCode::Tab, KeyModifiers::SHIFT)),
            ("enter", one(KeyCode::Enter, none)),
            ("esc", one(KeyCode::Esc, none)),
            ("tab", one(KeyCode::Tab, none)),
            ("space", one(KeyCode::Char(' '), none)),
            ("up", one(KeyCode::Up, none)),
            ("down", one(KeyCode::Down, none)),
            ("gg", two('g', 'g')),
            ("gT", two('g', 'T')),
            (",s", two(',', 's')),
        ];
        for (text, expected) in cases {
            assert_eq!(
                text.parse::<KeySequence>().as_ref(),
                Ok(&expected),
                "sequence `{text}`",
            );
        }
    }

    #[test]
    fn parse_failures_name_the_bad_token() {
        let cases = [
            ("abc", "abc"),
            ("", ""),
            ("ctlr-d", "ctlr"),
            ("ctrl-xyz", "xyz"),
            ("meta-x", "meta"),
        ];
        for (text, token) in cases {
            let error = text
                .parse::<KeySequence>()
                .expect_err("sequence should not parse");
            assert_eq!(error.token, token, "sequence `{text}`");
            assert!(
                error.to_string().contains(token),
                "message for `{text}` should name `{token}`",
            );
        }
    }

    #[test]
    fn chord_of_strips_shift_from_character_keys() {
        let upper =
            KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(Chord::of(upper), Chord::plain(KeyCode::Char('G')),);

        let shifted_tab =
            KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        assert_eq!(
            Chord::of(shifted_tab).modifiers,
            KeyModifiers::SHIFT,
        );
    }
}
