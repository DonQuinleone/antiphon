use antiphon_render::Link;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::app::App;

const PICKER_WIDTH: u16 = 60;
const PICKER_MAX_ROWS: u16 = 16;
const BORDER_ROWS: u16 = 2;
const ALLOWED_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LinkPicker {
    pub selected: usize,
    pub digits: String,
}

/// Keys while the picker is open. A returned url is the one
/// to hand to the system opener; everything else mutates the
/// picker in place.
pub(super) fn feed(app: &mut App, key: KeyEvent) -> Option<String> {
    let count = app.pager_rendered.links.len();
    let picker = app.link_picker.as_mut()?;
    match key.code {
        KeyCode::Esc => {
            app.link_picker = None;
            None
        }
        KeyCode::Char('j') | KeyCode::Down => {
            picker.selected =
                (picker.selected + 1).min(count.saturating_sub(1));
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            picker.selected = picker.selected.saturating_sub(1);
            None
        }
        KeyCode::Char(digit @ '0'..='9') => {
            picker.digits.push(digit);
            None
        }
        KeyCode::Backspace => {
            picker.digits.pop();
            None
        }
        KeyCode::Enter => submit(app),
        _ => None,
    }
}

fn submit(app: &mut App) -> Option<String> {
    let picker = app.link_picker.take()?;
    let links = &app.pager_rendered.links;
    let Some(link) = chosen(&picker, links) else {
        app.notice = Some(format!("no link {}", picker.digits));
        return None;
    };
    Some(link.url.clone())
}

fn chosen<'a>(
    picker: &LinkPicker,
    links: &'a [Link],
) -> Option<&'a Link> {
    if picker.digits.is_empty() {
        return links.get(picker.selected);
    }
    let number: usize = picker.digits.parse().ok()?;
    links.get(number.checked_sub(1)?)
}

/// Only web and mail urls ever reach the system opener; the
/// client never fetches anything itself.
pub(super) fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ALLOWED_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

pub(super) fn open_url(app: &mut App, url: &str) {
    if !openable(url) {
        app.notice = Some(format!("refusing to open {url}"));
        return;
    }
    spawn_opener(app, url);
}

pub(super) fn spawn_opener(app: &mut App, target: &str) {
    let spawned = std::process::Command::new(OPENER)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    app.notice = Some(match spawned {
        Ok(_) => format!("opening {target}"),
        Err(error) => format!("{OPENER}: {error}"),
    });
}

pub(super) fn draw_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.link_picker else {
        return;
    };
    let theme = app.theme;
    let links = &app.pager_rendered.links;
    let width = PICKER_WIDTH.min(area.width.saturating_sub(2));
    let height = (links.len() as u16 + BORDER_ROWS)
        .min(PICKER_MAX_ROWS)
        .min(area.height.saturating_sub(2));
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let block = Block::bordered()
        .title(" links ")
        .title_bottom(bottom_hint(picker))
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let lines: Vec<Line<'static>> = links
        .iter()
        .enumerate()
        .map(|(index, link)| {
            link_row(app, link, index == picker.selected)
        })
        .collect();
    let visible = height.saturating_sub(BORDER_ROWS) as usize;
    let scroll = (picker.selected + 1).saturating_sub(visible) as u16;
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll, 0)),
        modal,
    );
}

fn bottom_hint(picker: &LinkPicker) -> String {
    if picker.digits.is_empty() {
        return " number+enter \u{b7} j/k \u{b7} esc ".to_string();
    }
    format!(" open: {} ", picker.digits)
}

fn link_row(app: &App, link: &Link, selected: bool) -> Line<'static> {
    let theme = app.theme;
    let mut number = Style::new().fg(theme.accent_strong);
    let mut text = Style::new().fg(theme.text_primary);
    if selected {
        number = number.bg(theme.selection_bg);
        text = text.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let mut spans =
        vec![Span::styled(format!(" [{}] ", link.index), number)];
    if !link.label.is_empty() && link.label != link.url {
        spans.push(Span::styled(
            format!("{} ", link.label.trim()),
            text,
        ));
    }
    spans.push(Span::styled(link.url.clone(), text));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use antiphon_pgp::Signature;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    use super::super::app::View;
    use super::super::testkit::app_with_messages;
    use super::*;

    const LINKED_BODY: &str = "read https://example.com/a\n\
        then https://example.com/b\n\
        or https://example.com/c\n";

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn picker_app() -> App {
        let mut app = app_with_messages(1);
        app.open_pager(
            LINKED_BODY.to_string(),
            Signature::none(),
            Vec::new(),
        );
        app.link_picker = Some(LinkPicker::default());
        app
    }

    #[test]
    fn picker_keys_follow_the_table() {
        let cases: &[(&[KeyCode], Option<&str>, bool)] = &[
            (&[KeyCode::Esc], None, false),
            (&[KeyCode::Enter], Some("https://example.com/a"), false),
            (
                &[KeyCode::Char('j'), KeyCode::Enter],
                Some("https://example.com/b"),
                false,
            ),
            (
                &[
                    KeyCode::Char('j'),
                    KeyCode::Char('j'),
                    KeyCode::Char('j'),
                    KeyCode::Char('k'),
                    KeyCode::Enter,
                ],
                Some("https://example.com/b"),
                false,
            ),
            (
                &[KeyCode::Char('3'), KeyCode::Enter],
                Some("https://example.com/c"),
                false,
            ),
            (
                &[
                    KeyCode::Char('1'),
                    KeyCode::Backspace,
                    KeyCode::Char('2'),
                    KeyCode::Enter,
                ],
                Some("https://example.com/b"),
                false,
            ),
            (&[KeyCode::Char('9'), KeyCode::Enter], None, false),
            (&[KeyCode::Char('x')], None, true),
        ];
        for (presses, opened, stays_open) in cases {
            let mut app = picker_app();
            let mut url = None;
            for code in *presses {
                url = feed(&mut app, key(*code));
            }
            assert_eq!(url.as_deref(), *opened, "{presses:?}");
            assert_eq!(
                app.link_picker.is_some(),
                *stays_open,
                "{presses:?}"
            );
        }
    }

    #[test]
    fn a_bad_number_closes_with_a_notice() {
        let mut app = picker_app();
        feed(&mut app, key(KeyCode::Char('9')));
        let url = feed(&mut app, key(KeyCode::Enter));
        assert_eq!(url, None);
        assert_eq!(app.notice.as_deref(), Some("no link 9"));
    }

    #[test]
    fn only_web_and_mail_schemes_open() {
        let cases = [
            ("https://example.com", true),
            ("HTTP://EXAMPLE.COM", true),
            ("mailto:mara@example.com", true),
            ("file:///etc/passwd", false),
            ("javascript:alert(1)", false),
            ("ftp://example.com", false),
            ("example.com", false),
        ];
        for (url, allowed) in cases {
            assert_eq!(openable(url), allowed, "{url}");
        }
        let mut app = picker_app();
        open_url(&mut app, "file:///etc/passwd");
        assert_eq!(
            app.notice.as_deref(),
            Some("refusing to open file:///etc/passwd")
        );
    }

    #[test]
    fn the_modal_lists_numbered_links() {
        let app = picker_app();
        assert_eq!(app.view, View::Pager);
        let backend = TestBackend::new(70, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                super::super::draw::draw(frame, &app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..20)
            .map(|y| {
                (0..70)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .unwrap()
                            .symbol()
                            .to_string()
                    })
                    .collect()
            })
            .collect();
        assert!(
            rows.iter().any(|row| row.contains(" links ")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("[1] https://example.com/a")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("[3] https://example.com/c")),
            "{rows:?}"
        );
    }
}
