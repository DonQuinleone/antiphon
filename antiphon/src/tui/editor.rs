use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use portable_pty::{
    Child, CommandBuilder, MasterPty, PtySize, native_pty_system,
};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseEventKind,
};

const PTY_READ_CHUNK: usize = 4096;
const SCROLLBACK_LINES: usize = 0;
const EDITOR_TERM: &str = "xterm-256color";
const ESC: u8 = 0x1b;
const BACKSPACE_DEL: u8 = 0x7f;
const CTRL_MASK: u8 = 0x1f;

/// SGR mouse report frame: `ESC [ < button ; col ; row M`,
/// with one-based coordinates. Only the press form (`M`) is
/// used, which is what a wheel notch carries.
const SGR_MOUSE_INTRO: &str = "\x1b[<";
const SGR_MOUSE_PRESS: char = 'M';
const SGR_WHEEL_UP: u16 = 64;
const SGR_WHEEL_DOWN: u16 = 65;

/// The body file handed to the embedded editor and the live
/// pty session, settled once the child exits; everything else
/// about the compose lives in the compose state.
pub struct EditorPane {
    pub path: PathBuf,
    pub session: EditorSession,
}

/// The live pty: the child runs the user's editor, a reader
/// thread feeds its output through a channel into the vt100
/// parser, and key events go back down as encoded bytes.
pub struct EditorSession {
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Receiver<Vec<u8>>,
    rows: u16,
    cols: u16,
}

impl EditorSession {
    pub fn spawn(
        editor: &str,
        draft: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<EditorSession, String> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(pty_size(rows, cols))
            .map_err(|error| error.to_string())?;
        let mut command = CommandBuilder::new("sh");
        command.arg("-c");
        command.arg(format!("{editor} \"$0\""));
        command.arg(draft);
        command.env("TERM", EDITOR_TERM);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        Ok(EditorSession {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_LINES),
            master: pair.master,
            writer,
            child,
            output: read_on_thread(reader),
            rows,
            cols,
        })
    }

    pub fn pump(&mut self) {
        while let Ok(bytes) = self.output.try_recv() {
            self.parser.process(&bytes);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        if self.master.resize(pty_size(rows, cols)).is_err() {
            return;
        }
        self.parser.screen_mut().set_size(rows, cols);
        self.rows = rows;
        self.cols = cols;
    }

    pub fn send_key(&mut self, key: KeyEvent) {
        let bytes = encode_key(key);
        if bytes.is_empty() {
            return;
        }
        let _ = self.writer.write_all(&bytes);
        let _ = self.writer.flush();
    }

    /// Whether the editor has asked for SGR mouse reporting, so
    /// a wheel notch can be forwarded rather than typed into the
    /// buffer as stray escape bytes when the mouse is off.
    pub fn wants_mouse(&self) -> bool {
        let screen = self.parser.screen();
        screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None
            && screen.mouse_protocol_encoding()
                == vt100::MouseProtocolEncoding::Sgr
    }

    /// Forwards a mouse event as an SGR report at the given
    /// pane-relative, zero-based cell, so the editor scrolls
    /// natively as it would in a standalone terminal.
    pub fn send_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16) {
        let bytes = encode_mouse(kind, col, row);
        if bytes.is_empty() {
            return;
        }
        let _ = self.writer.write_all(&bytes);
        let _ = self.writer.flush();
    }

    /// Ends the child outright; used when the compose that
    /// owned this editor is discarded rather than finished.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn exit_success(&mut self) -> Option<bool> {
        let status = self.child.try_wait().ok().flatten()?;
        Some(status.success())
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn read_on_thread(
    mut reader: Box<dyn Read + Send>,
) -> Receiver<Vec<u8>> {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; PTY_READ_CHUNK];
        loop {
            let Ok(count) = reader.read(&mut buffer) else {
                return;
            };
            if count == 0 {
                return;
            }
            if sender.send(buffer[..count].to_vec()).is_err() {
                return;
            }
        }
    });
    receiver
}

const NAMED_KEYS: &[(KeyCode, &[u8])] = &[
    (KeyCode::Enter, b"\r"),
    (KeyCode::Esc, &[ESC]),
    (KeyCode::Backspace, &[BACKSPACE_DEL]),
    (KeyCode::Tab, b"\t"),
    (KeyCode::BackTab, b"\x1b[Z"),
    (KeyCode::Up, b"\x1b[A"),
    (KeyCode::Down, b"\x1b[B"),
    (KeyCode::Right, b"\x1b[C"),
    (KeyCode::Left, b"\x1b[D"),
    (KeyCode::Home, b"\x1b[H"),
    (KeyCode::End, b"\x1b[F"),
    (KeyCode::Insert, b"\x1b[2~"),
    (KeyCode::Delete, b"\x1b[3~"),
    (KeyCode::PageUp, b"\x1b[5~"),
    (KeyCode::PageDown, b"\x1b[6~"),
];

pub fn encode_key(key: KeyEvent) -> Vec<u8> {
    let mut bytes = base_bytes(&key);
    if bytes.is_empty() {
        return bytes;
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, ESC);
    }
    bytes
}

fn base_bytes(key: &KeyEvent) -> Vec<u8> {
    if let KeyCode::Char(ch) = key.code {
        return char_bytes(ch, key.modifiers);
    }
    NAMED_KEYS
        .iter()
        .find(|(code, _)| *code == key.code)
        .map(|(_, bytes)| bytes.to_vec())
        .unwrap_or_default()
}

fn char_bytes(ch: char, modifiers: KeyModifiers) -> Vec<u8> {
    if modifiers.contains(KeyModifiers::CONTROL) {
        return ctrl_bytes(ch);
    }
    let mut buffer = [0u8; 4];
    ch.encode_utf8(&mut buffer).as_bytes().to_vec()
}

fn ctrl_bytes(ch: char) -> Vec<u8> {
    let lowered = ch.to_ascii_lowercase();
    if !lowered.is_ascii() {
        return Vec::new();
    }
    vec![(lowered as u8) & CTRL_MASK]
}

/// The SGR mouse report for a forwarded event, at one-based
/// coordinates. Only wheel notches are forwarded; every other
/// kind encodes to nothing and is dropped.
pub fn encode_mouse(kind: MouseEventKind, col: u16, row: u16) -> Vec<u8> {
    let Some(button) = wheel_button(kind) else {
        return Vec::new();
    };
    format!(
        "{SGR_MOUSE_INTRO}{button};{};{}{SGR_MOUSE_PRESS}",
        col + 1,
        row + 1
    )
    .into_bytes()
}

fn wheel_button(kind: MouseEventKind) -> Option<u16> {
    match kind {
        MouseEventKind::ScrollUp => Some(SGR_WHEEL_UP),
        MouseEventKind::ScrollDown => Some(SGR_WHEEL_DOWN),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn keys_encode_to_their_terminal_bytes() {
        let cases: &[(KeyCode, KeyModifiers, &[u8])] = &[
            (KeyCode::Char('a'), KeyModifiers::NONE, b"a"),
            (KeyCode::Char('Z'), KeyModifiers::SHIFT, b"Z"),
            (
                KeyCode::Char('\u{fc}'),
                KeyModifiers::NONE,
                "\u{fc}".as_bytes(),
            ),
            (KeyCode::Enter, KeyModifiers::NONE, b"\r"),
            (KeyCode::Esc, KeyModifiers::NONE, b"\x1b"),
            (KeyCode::Backspace, KeyModifiers::NONE, b"\x7f"),
            (KeyCode::Tab, KeyModifiers::NONE, b"\t"),
            (KeyCode::Up, KeyModifiers::NONE, b"\x1b[A"),
            (KeyCode::Down, KeyModifiers::NONE, b"\x1b[B"),
            (KeyCode::Right, KeyModifiers::NONE, b"\x1b[C"),
            (KeyCode::Left, KeyModifiers::NONE, b"\x1b[D"),
            (KeyCode::Home, KeyModifiers::NONE, b"\x1b[H"),
            (KeyCode::End, KeyModifiers::NONE, b"\x1b[F"),
            (KeyCode::Delete, KeyModifiers::NONE, b"\x1b[3~"),
            (KeyCode::PageUp, KeyModifiers::NONE, b"\x1b[5~"),
            (KeyCode::PageDown, KeyModifiers::NONE, b"\x1b[6~"),
            (KeyCode::Char('c'), KeyModifiers::CONTROL, b"\x03"),
            (KeyCode::Char('D'), KeyModifiers::CONTROL, b"\x04"),
            (KeyCode::Char('x'), KeyModifiers::ALT, b"\x1bx"),
            (KeyCode::F(1), KeyModifiers::NONE, b""),
        ];
        for (code, modifiers, expected) in cases {
            assert_eq!(
                encode_key(key(*code, *modifiers)),
                *expected,
                "{code:?} {modifiers:?}"
            );
        }
    }

    #[test]
    fn wheel_notches_encode_to_sgr_mouse_reports() {
        let cases: &[(MouseEventKind, u16, u16, &[u8])] = &[
            (MouseEventKind::ScrollUp, 9, 4, b"\x1b[<64;10;5M"),
            (MouseEventKind::ScrollDown, 9, 4, b"\x1b[<65;10;5M"),
            (MouseEventKind::ScrollDown, 0, 0, b"\x1b[<65;1;1M"),
        ];
        for (kind, col, row, expected) in cases {
            assert_eq!(
                encode_mouse(*kind, *col, *row),
                *expected,
                "{kind:?} at ({col},{row})"
            );
        }
    }

    #[test]
    fn non_wheel_mouse_kinds_encode_to_nothing() {
        assert!(
            encode_mouse(MouseEventKind::Moved, 3, 3).is_empty(),
            "movement is not forwarded"
        );
    }
}
