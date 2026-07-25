use antiphon_store::MessageSummary;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row, Table, TableState};

use super::app::App;
use super::cells::message_row;
pub(super) use super::cells::{UNREAD_MARK, format_date, sender_name};

pub(super) const MARK_COLS: u16 = 4;
const FROM_WIDTH: u16 = 24;
const COLUMN_GAP: u16 = 1;
const DATE_HEADING: &str = "DATE";
const SUBJECT_HEADING: &str = "SUBJECT";
const FROM_TO_HEADING: &str = "FROM/TO";
const STATUS_COLS: u16 = 3;
const STATUS_HEADING: &str = "#";

/// One column layout shared by the header row and every
/// message row, so the two can never drift apart.
pub(super) struct Columns {
    pub(super) date: u16,
    pub(super) from: u16,
    pub(super) subject: u16,
}

fn columns(
    width: u16,
    messages: &[MessageSummary],
    format: &str,
) -> Columns {
    let date = date_width(messages, format);
    let taken = STATUS_COLS + date + FROM_WIDTH + 3 * COLUMN_GAP;
    Columns {
        date,
        from: FROM_WIDTH,
        subject: width.saturating_sub(taken),
    }
}

/// Exactly as wide as the widest rendered date, so nothing is
/// ever chopped, with the heading as the floor.
fn date_width(messages: &[MessageSummary], format: &str) -> u16 {
    messages
        .iter()
        .map(|message| {
            format_date(message.date_unix, format).chars().count()
        })
        .max()
        .unwrap_or(0)
        .max(DATE_HEADING.len()) as u16
}

pub(super) fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    if app.messages.is_empty() {
        draw_empty(frame, app, area);
        return;
    }
    let columns = columns(area.width, &app.messages, &app.date_format);
    let rows = app.messages.iter().map(|message| {
        message_row(app, &columns, message, &app.date_format)
    });
    // The subject cell opens with the unread/flag gutter; the
    // heading shifts by the same width so the word sits over
    // the text, not the markers.
    let subject_heading =
        format!("{:1$}{SUBJECT_HEADING}", "", MARK_COLS as usize);
    let header = Row::new(vec![
        STATUS_HEADING.to_string(),
        DATE_HEADING.to_string(),
        FROM_TO_HEADING.to_string(),
        subject_heading,
    ])
    .style(
        Style::new()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(STATUS_COLS),
            Constraint::Length(columns.date),
            Constraint::Length(columns.from),
            Constraint::Min(0),
        ],
    )
    .column_spacing(COLUMN_GAP)
    .header(header)
    .row_highlight_style(
        Style::new().bg(theme.selection_bg).fg(theme.selection_fg),
    );
    let mut state =
        TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);
}

/// An empty view keeps its header row so the shape of the
/// list never jumps, with the notice centred in the space
/// the rows would fill.
fn draw_empty(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let columns = columns(area.width, &[], &app.date_format);
    let subject_heading =
        format!("{:1$}{SUBJECT_HEADING}", "", MARK_COLS as usize);
    let header = Row::new(vec![
        STATUS_HEADING.to_string(),
        DATE_HEADING.to_string(),
        FROM_TO_HEADING.to_string(),
        subject_heading,
    ])
    .style(
        Style::new()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        std::iter::empty::<Row>(),
        [
            Constraint::Length(STATUS_COLS),
            Constraint::Length(columns.date),
            Constraint::Length(columns.from),
            Constraint::Min(0),
        ],
    )
    .column_spacing(COLUMN_GAP)
    .header(header);
    frame.render_widget(table, area);
    let label = app.active_search.as_deref().unwrap_or("this view");
    let notice = Paragraph::new(Line::from(Span::styled(
        format!("nothing in {label}"),
        Style::new().fg(theme.text_muted),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    let row = Rect {
        x: area.x,
        y: area.y + 1 + area.height.saturating_sub(1) / 2,
        width: area.width,
        height: 1,
    };
    frame.render_widget(notice, row);
}

#[cfg(test)]
mod tests {
    use super::super::cells::{ELLIPSIS, truncate};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::super::testkit::app_with_messages;
    use super::*;

    const ISO: &str = "%Y-%m-%d %H:%M";
    const NOON_ISH: i64 = 1_768_000_000;

    fn listed_app(rows: &[(&str, &str)], format: &str) -> App {
        let mut app = app_with_messages(rows.len());
        app.date_format = format.to_string();
        for (index, (from, subject)) in rows.iter().enumerate() {
            let message = &mut app.messages[index];
            message.from = (*from).to_string();
            message.subject = (*subject).to_string();
            message.date_unix = NOON_ISH + index as i64;
        }
        app
    }

    fn render(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_list(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_chars(buffer: &Buffer, y: u16) -> Vec<char> {
        (0..buffer.area.width)
            .map(|x| {
                buffer
                    .cell((x, y))
                    .unwrap()
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect()
    }

    fn column_of(chars: &[char], needle: &str) -> Option<usize> {
        let wanted: Vec<char> = needle.chars().collect();
        chars
            .windows(wanted.len())
            .position(|window| window == wanted)
    }

    fn foreground(
        buffer: &Buffer,
        x: u16,
        y: u16,
    ) -> Option<ratatui::style::Color> {
        buffer.cell((x, y)).unwrap().style().fg
    }

    #[test]
    fn header_and_rows_share_one_column_layout() {
        let app = listed_app(
            &[
                ("Mara Voss <mara@example.com>", "hello there"),
                ("Bo <bo@example.com>", "meeting notes"),
            ],
            ISO,
        );
        let status_x = STATUS_COLS as usize + COLUMN_GAP as usize;
        let date_cols = format_date(NOON_ISH, ISO).chars().count();
        let from_x = status_x + date_cols + COLUMN_GAP as usize;
        let subject_x =
            from_x + FROM_WIDTH as usize + COLUMN_GAP as usize;

        let buffer = render(&app, 80, 6);
        let header = row_chars(&buffer, 0);
        assert_eq!(column_of(&header, DATE_HEADING), Some(status_x));
        assert_eq!(column_of(&header, FROM_TO_HEADING), Some(from_x));
        assert_eq!(
            column_of(&header, SUBJECT_HEADING),
            Some(subject_x + MARK_COLS as usize)
        );

        let names = ["Mara Voss", "Bo"];
        let subjects = ["hello there", "meeting notes"];
        for (index, (name, subject)) in
            names.iter().zip(subjects).enumerate()
        {
            let y = (index + 1) as u16;
            let row = row_chars(&buffer, y);
            let date = format_date(app.messages[index].date_unix, ISO);
            assert_eq!(
                column_of(&row, &date),
                Some(status_x),
                "row {y}"
            );
            assert_eq!(column_of(&row, name), Some(from_x), "row {y}");
            assert_eq!(
                column_of(&row, subject),
                Some(subject_x + MARK_COLS as usize),
                "row {y}"
            );
        }
    }

    #[test]
    fn overlong_cells_end_in_an_ellipsis_not_the_gutter() {
        let long_from =
            "Maximiliana Verylongname Voss <mv@example.com>";
        let long_subject = "a very long subject line that cannot \
                            possibly fit in what remains of the row";
        let app = listed_app(&[(long_from, long_subject)], ISO);
        let buffer = render(&app, 64, 4);
        let row = row_chars(&buffer, 1);

        let from_x = STATUS_COLS as usize
            + COLUMN_GAP as usize
            + format_date(NOON_ISH, ISO).chars().count()
            + COLUMN_GAP as usize;
        let from_end = from_x + FROM_WIDTH as usize - 1;
        assert_eq!(row[from_end], ELLIPSIS, "{row:?}");
        assert_eq!(row[from_end + 1], ' ', "gutter must stay clear");
        assert_eq!(row[63], ELLIPSIS, "{row:?}");
    }

    #[test]
    fn the_date_and_time_wear_their_own_colours() {
        let app = listed_app(
            &[
                ("a <a@example.com>", "one"),
                ("b <b@example.com>", "two"),
            ],
            ISO,
        );
        let buffer = render(&app, 80, 6);
        let rendered = format_date(NOON_ISH + 1, ISO);
        let split = rendered.rfind(' ').unwrap();
        let theme = app.theme;
        let offset = STATUS_COLS as usize + COLUMN_GAP as usize;
        for x in 0..split {
            assert_eq!(
                foreground(&buffer, (offset + x) as u16, 2),
                Some(theme.list_date),
                "column {x}"
            );
        }
        for x in (split + 1)..rendered.chars().count() {
            assert_eq!(
                foreground(&buffer, (offset + x) as u16, 2),
                Some(theme.list_time),
                "column {x}"
            );
        }
    }

    #[test]
    fn a_single_token_format_is_all_date_coloured() {
        let app = listed_app(
            &[("a <a@example.com>", "one"), ("b", "two")],
            "%Y",
        );
        let buffer = render(&app, 40, 6);
        let rendered = format_date(NOON_ISH + 1, "%Y");
        let offset = STATUS_COLS as usize + COLUMN_GAP as usize;
        for x in 0..rendered.chars().count() {
            assert_eq!(
                foreground(&buffer, (offset + x) as u16, 2),
                Some(app.theme.list_date),
                "column {x}"
            );
        }
    }

    #[test]
    fn from_and_subject_wear_their_column_colours() {
        let app = listed_app(
            &[
                ("a <a@example.com>", "one"),
                ("Bo <b@example.com>", "two"),
            ],
            ISO,
        );
        let buffer = render(&app, 80, 6);
        let row = row_chars(&buffer, 2);
        let from_x = column_of(&row, "Bo").unwrap() as u16;
        let subject_x = column_of(&row, "two").unwrap() as u16;
        assert_eq!(
            foreground(&buffer, from_x, 2),
            Some(app.theme.list_from)
        );
        assert_eq!(
            foreground(&buffer, subject_x, 2),
            Some(app.theme.list_subject)
        );
    }

    #[test]
    fn truncation_is_exact_at_every_width() {
        let cases = [
            ("hello", 10, "hello".to_string()),
            ("hello", 5, "hello".to_string()),
            ("hello!", 5, format!("hell{ELLIPSIS}")),
            ("hi", 1, ELLIPSIS.to_string()),
            ("hi", 0, String::new()),
        ];
        for (text, width, expected) in cases {
            assert_eq!(
                truncate(text, width),
                expected,
                "{text}@{width}"
            );
        }
    }

    #[test]
    fn an_empty_list_keeps_headers_and_centres_the_notice() {
        let app = listed_app(&[], ISO);
        let buffer = render(&app, 40, 5);
        let header: String =
            row_chars(&buffer, 0).into_iter().collect();
        assert!(header.contains(DATE_HEADING), "{header:?}");
        assert!(header.contains(STATUS_HEADING), "{header:?}");
        let middle: String =
            row_chars(&buffer, 3).into_iter().collect();
        assert!(
            middle.contains("nothing in"),
            "expected the centred notice, got {middle:?}"
        );
        let leading = middle.len() - middle.trim_start().len();
        assert!(
            leading > 2,
            "notice should be centred, got {middle:?}"
        );
    }

    #[test]
    fn sender_names_prefer_the_display_part() {
        let cases = [
            ("Mara Voss <mara@example.com>", "Mara Voss"),
            ("\"Voss, Mara\" <mara@example.com>", "Voss, Mara"),
            ("mara@example.com", "mara@example.com"),
            ("<mara@example.com>", "<mara@example.com>"),
        ];
        for (from, expected) in cases {
            assert_eq!(sender_name(from), expected, "{from}");
        }
    }

    #[test]
    fn dates_format_per_the_config_pattern() {
        let formatted = format_date(1_764_671_045, "%Y");
        assert_eq!(formatted, "2025");
        assert_eq!(format_date(i64::MAX, "%Y"), "");
    }
}
