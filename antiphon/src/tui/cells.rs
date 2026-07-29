use antiphon_store::MessageSummary;
use antiphon_ui::{Theme, truncate};
use chrono::{DateTime, Local};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Row;

use super::app::App;
use super::message_list::{Columns, MARK_COLS};

pub(super) const UNREAD_MARK: &str = "\u{25c6} ";
const READ_MARK: &str = "  ";
const FLAG_MARK: &str = "\u{2691} ";
const NO_FLAG_MARK: &str = "  ";
const REPLIED_TAG: &str = "replied";
const FORWARDED_TAG: &str = "passed";
const ATTACHMENT_TAG: &str = "attachment";
const OWN_MAIL_PREFIX: &str = "\u{2192} ";
const FLAGGED_TAG: &str = "flagged";

pub(super) fn message_row(
    app: &App,
    columns: &Columns,
    message: &MessageSummary,
    format: &str,
    threaded: bool,
) -> Row<'static> {
    let theme = app.theme;
    Row::new(vec![
        status_cell(theme, message, threaded),
        date_cell(theme, format_date(message.date_unix, format)),
        from_cell(app, columns, message),
        subject_cell(theme, columns, message),
    ])
}

/// R replied, F forwarded, A attachment (from the tags the
/// store already keeps), T when the window shows more of the
/// same thread.
fn status_cell(
    theme: &Theme,
    message: &MessageSummary,
    threaded: bool,
) -> Line<'static> {
    let has = |tag: &str| {
        message.tags.iter().any(|candidate| candidate == tag)
    };
    let text = format!(
        "{}{}{}{}",
        if has(REPLIED_TAG) { 'R' } else { ' ' },
        if has(FORWARDED_TAG) { 'F' } else { ' ' },
        if has(ATTACHMENT_TAG) { 'A' } else { ' ' },
        if threaded { 'T' } else { ' ' },
    );
    Line::from(Span::styled(text, Style::new().fg(theme.text_muted)))
}

fn date_cell(theme: &Theme, rendered: String) -> Line<'static> {
    Line::from(date_spans(theme, rendered))
}

/// The rendered date splits at its last space: the left part
/// takes the date colour, the right the time colour, whatever
/// strftime format the user set. A single-token format is all
/// date.
pub(super) fn date_spans(
    theme: &Theme,
    rendered: String,
) -> Vec<Span<'static>> {
    let Some(split) = rendered.rfind(' ') else {
        return vec![Span::styled(
            rendered,
            Style::new().fg(theme.list_date),
        )];
    };
    let date = rendered[..split].to_string();
    let time = rendered[split..].to_string();
    vec![
        Span::styled(date, Style::new().fg(theme.list_date)),
        Span::styled(time, Style::new().fg(theme.list_time)),
    ]
}

fn from_cell(
    app: &App,
    columns: &Columns,
    message: &MessageSummary,
) -> Line<'static> {
    let theme = app.theme;
    let text = if app.is_own(&message.from) {
        format!("{OWN_MAIL_PREFIX}{}", sender_name(&message.to))
    } else {
        sender_name(&message.from)
    };
    let name = truncate(&text, columns.from as usize);
    let style = Style::new().fg(theme.list_from);
    Line::from(Span::styled(name, unread_weight(style, message)))
}

fn subject_cell(
    theme: &Theme,
    columns: &Columns,
    message: &MessageSummary,
) -> Line<'static> {
    let unread = if message.unread {
        Span::styled(UNREAD_MARK, Style::new().fg(theme.unread_marker))
    } else {
        Span::raw(READ_MARK)
    };
    let flagged = if is_flagged(message) {
        Span::styled(FLAG_MARK, Style::new().fg(theme.accent_strong))
    } else {
        Span::raw(NO_FLAG_MARK)
    };
    let width = columns.subject.saturating_sub(MARK_COLS);
    let subject = truncate(&message.subject, width as usize);
    let style = Style::new().fg(theme.list_subject);
    Line::from(vec![
        unread,
        flagged,
        Span::styled(subject, unread_weight(style, message)),
    ])
}

fn unread_weight(style: Style, message: &MessageSummary) -> Style {
    if message.unread {
        return style.add_modifier(Modifier::BOLD);
    }
    style
}

fn is_flagged(message: &MessageSummary) -> bool {
    message.tags.iter().any(|tag| tag == FLAGGED_TAG)
}

pub(super) fn sender_name(from: &str) -> String {
    let name = from.split('<').next().unwrap_or("").trim();
    let name = name.trim_matches('"');
    if name.is_empty() {
        return from.trim().to_string();
    }
    name.to_string()
}

pub(super) fn format_date(unix: i64, format: &str) -> String {
    let Some(utc) = DateTime::from_timestamp(unix, 0) else {
        return String::new();
    };
    utc.with_timezone(&Local).format(format).to_string()
}
