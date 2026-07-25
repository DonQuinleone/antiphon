use std::path::PathBuf;

use antiphon_render::MessageAttachment;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::attach::expand_tilde;
use super::commands::PromptKind;
use super::link_picker::spawn_opener;
use super::pager::fitted;

const COLLAPSED_ROWS: u16 = 1;
const HEADER_ROWS: u16 = 1;
const LIST_ROWS_MAX: u16 = 8;
const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const HINT: &str = "attachments \u{b7} j/k select \u{b7} \
                    s save \u{b7} v view \u{b7} esc close";

/// Rows the drawer takes at the pager's bottom: nothing
/// without attachments, one summary line collapsed, a header
/// and a capped list expanded.
pub(super) fn rows_needed(app: &App) -> u16 {
    let count = app.pager_attachments.len() as u16;
    if count == 0 {
        return 0;
    }
    if !app.drawer_open {
        return COLLAPSED_ROWS;
    }
    HEADER_ROWS + count.min(LIST_ROWS_MAX)
}

pub(super) fn draw_drawer(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let lines = if app.drawer_open {
        expanded_lines(app, area)
    } else {
        vec![summary_line(app, area.width)]
    };
    frame.render_widget(Paragraph::new(lines), area);
}

fn summary_line(app: &App, width: u16) -> Line<'static> {
    let theme = app.theme;
    let count = app.pager_attachments.len();
    let noun = if count == 1 {
        "attachment"
    } else {
        "attachments"
    };
    let names: Vec<&str> = app
        .pager_attachments
        .iter()
        .map(|attachment| attachment.filename.as_str())
        .collect();
    let text = format!("{count} {noun}: {}", names.join(", "));
    Line::from(Span::styled(
        format!(
            "{:<width$}",
            fitted(&text, width as usize),
            width = width as usize
        ),
        Style::new().fg(theme.text_muted).bg(theme.surface),
    ))
}

fn expanded_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    let theme = app.theme;
    let mut lines = vec![Line::from(Span::styled(
        format!("{HINT:<width$}", width = area.width as usize),
        Style::new()
            .fg(theme.text_primary)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD),
    ))];
    let visible = area.height.saturating_sub(HEADER_ROWS) as usize;
    let first = (app.drawer_selected + 1).saturating_sub(visible);
    let shown = app
        .pager_attachments
        .iter()
        .enumerate()
        .skip(first)
        .take(visible);
    for (index, attachment) in shown {
        lines.push(attachment_line(
            theme,
            attachment,
            index == app.drawer_selected,
        ));
    }
    lines
}

fn attachment_line(
    theme: &antiphon_ui::Theme,
    attachment: &MessageAttachment,
    selected: bool,
) -> Line<'static> {
    let marker = if selected {
        SELECTED_MARK
    } else {
        UNSELECTED_MARK
    };
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.fg(theme.accent_strong);
    }
    Line::from(Span::styled(
        format!("{marker}{}", attachment.label()),
        style,
    ))
}

/// Keys while the drawer is expanded; everything else is
/// swallowed so the pager underneath stays put.
pub(super) fn feed(app: &mut App, key: KeyEvent) {
    let last = app.pager_attachments.len().saturating_sub(1);
    match key.code {
        KeyCode::Esc => app.drawer_open = false,
        KeyCode::Char('j') | KeyCode::Down => {
            app.drawer_selected = (app.drawer_selected + 1).min(last)
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.drawer_selected = app.drawer_selected.saturating_sub(1)
        }
        KeyCode::Char('s') => prompt_save(app),
        KeyCode::Char('v') => view_selected(app),
        _ => {}
    }
}

/// s asks where to save, prefilled with the sent filename so
/// enter alone drops it in the working directory.
fn prompt_save(app: &mut App) {
    let Some(attachment) = selected(app) else {
        return;
    };
    let filename = attachment.filename.clone();
    app.open_prompt(PromptKind::SaveAttachment);
    for ch in filename.chars() {
        app.prompt_push(ch);
    }
}

fn selected(app: &App) -> Option<&MessageAttachment> {
    app.pager_attachments.get(app.drawer_selected)
}

pub(super) fn save_selected(app: &mut App, input: &str) {
    let Some(attachment) = selected(app) else {
        return;
    };
    let path = expand_tilde(input.trim());
    match std::fs::write(&path, &attachment.bytes) {
        Ok(()) => {
            app.notice = Some(format!("saved {}", path.display()));
        }
        Err(error) => {
            app.notice =
                Some(format!("save {}: {error}", path.display()));
        }
    }
}

/// v writes the decoded bytes to a temporary file and hands
/// that file to the system opener; nothing is executed, the
/// opener decides the viewer.
fn view_selected(app: &mut App) {
    let Some(attachment) = selected(app) else {
        return;
    };
    let path = match write_temp(attachment) {
        Ok(path) => path,
        Err(error) => {
            app.notice = Some(format!("view: {error}"));
            return;
        }
    };
    spawn_opener(app, &path.to_string_lossy());
}

fn write_temp(
    attachment: &MessageAttachment,
) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir()
        .join(format!("antiphon-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(&attachment.filename);
    std::fs::write(&path, &attachment.bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use antiphon_pgp::Signature;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::KeyModifiers;

    use super::super::testkit::app_with_messages;
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn attachment(name: &str, bytes: &[u8]) -> MessageAttachment {
        MessageAttachment {
            filename: name.to_string(),
            content_type: "application/pdf".to_string(),
            bytes: bytes.to_vec(),
        }
    }

    fn drawer_app() -> App {
        let mut app = app_with_messages(1);
        app.open_pager(
            "body\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        app.pager_attachments = vec![
            attachment("report.pdf", b"%PDF-1.7"),
            attachment("photo.jpg", b"\xff\xd8\xff"),
        ];
        app
    }

    fn rendered(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::super::draw::draw(frame, app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn collapsed_drawer_summarises_above_the_statusline() {
        let app = drawer_app();
        let buffer = rendered(&app, 60, 12);
        let drawer = row_text(&buffer, 10);
        assert!(
            drawer.starts_with("2 attachments: report.pdf, photo.jpg"),
            "{drawer:?}"
        );
        assert!(
            row_text(&buffer, 11).contains("messages"),
            "the statusline keeps the last row"
        );
    }

    #[test]
    fn a_narrow_summary_truncates_with_an_ellipsis() {
        let app = drawer_app();
        let line = summary_line(&app, 24);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.chars().count() <= 24, "{text:?}");
        assert!(text.contains('\u{2026}'), "{text:?}");
    }

    #[test]
    fn the_expanded_drawer_lists_and_selects() {
        let mut app = drawer_app();
        app.drawer_open = true;
        let buffer = rendered(&app, 60, 12);
        let rows: Vec<String> =
            (0..12).map(|y| row_text(&buffer, y)).collect();
        assert!(
            rows.iter().any(|row| row.starts_with("attachments")),
            "{rows:?}"
        );
        assert!(rows.iter().any(|row| row.starts_with(
            "\u{25b8} report.pdf (application/pdf, 8 bytes)"
        )));
        assert!(rows.iter().any(|row| row.starts_with("  photo.jpg")));

        feed(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.drawer_selected, 1);
        feed(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.drawer_selected, 1, "clamped at the end");
        feed(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.drawer_selected, 0);
        feed(&mut app, key(KeyCode::Esc));
        assert!(!app.drawer_open);
    }

    #[test]
    fn s_prompts_with_the_sent_filename() {
        let mut app = drawer_app();
        app.drawer_open = true;
        feed(&mut app, key(KeyCode::Char('s')));
        let prompt = app.prompt.as_ref().expect("a save prompt");
        assert_eq!(prompt.kind, PromptKind::SaveAttachment);
        assert_eq!(prompt.buffer, "report.pdf");
    }

    #[test]
    fn saving_writes_the_decoded_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.pdf");
        let mut app = drawer_app();
        save_selected(&mut app, target.to_str().unwrap());
        assert_eq!(std::fs::read(&target).unwrap(), b"%PDF-1.7");
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.starts_with("saved ")),
            "{:?}",
            app.notice
        );

        save_selected(&mut app, "/nonexistent/dir/out.pdf");
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.starts_with("save ")),
            "{:?}",
            app.notice
        );
    }

    #[test]
    fn viewing_writes_a_temp_copy() {
        let app = drawer_app();
        let attachment = selected(&app).unwrap();
        let path = write_temp(attachment).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.7");
        assert!(path.ends_with("report.pdf"));
        let _ = std::fs::remove_file(path);
    }
}
