//! The compose schedule prompt: a small modal on the review
//! screen taking either a relative delay (30m, 2h, 3d) or an
//! absolute local datetime (2026-08-01 09:00), resolved to the
//! outbox send-after time. Kept on `App` so `input.rs` can
//! intercept its keys the same way the alias modal does.

use antiphon_core::{Action, Context, Keymap, Resolution};
use chrono::{Local, NaiveDateTime, TimeZone};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::app::App;
use super::headers::{byte_index, with_cursor};

const MODAL_WIDTH: u16 = 52;
const HINT: &str = " enter schedules \u{b7} 2h/30m/3d or \
     2026-08-01 09:00 \u{b7} empty clears \u{b7} esc cancels ";
const DATE_FORMATS: [&str; 3] =
    ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S"];

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ScheduleEdit {
    pub(super) text: String,
    pub(super) cursor: usize,
}

pub(super) fn begin(app: &mut App) {
    app.schedule_edit = Some(ScheduleEdit::default());
}

/// esc cancels, enter parses and applies, everything else edits
/// the line.
pub(super) fn feed_edit(
    app: &mut App,
    keymap: &mut Keymap,
    key: KeyEvent,
) {
    match keymap.feed(Context::Prompt, key) {
        Resolution::Match(Action::PromptCancel) => {
            app.schedule_edit = None
        }
        Resolution::Match(Action::PromptSubmit) => apply(app),
        _ => edit_line(app, key),
    }
}

fn edit_line(app: &mut App, key: KeyEvent) {
    let Some(edit) = app.schedule_edit.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Char(ch) => {
            let at = byte_index(&edit.text, edit.cursor);
            edit.text.insert(at, ch);
            edit.cursor += 1;
        }
        KeyCode::Backspace => {
            if edit.cursor > 0 {
                edit.cursor -= 1;
                let at = byte_index(&edit.text, edit.cursor);
                edit.text.remove(at);
            }
        }
        KeyCode::Left => edit.cursor = edit.cursor.saturating_sub(1),
        KeyCode::Right => {
            edit.cursor =
                (edit.cursor + 1).min(edit.text.chars().count())
        }
        _ => {}
    }
}

fn apply(app: &mut App) {
    let text = match &app.schedule_edit {
        Some(edit) => edit.text.trim().to_string(),
        None => return,
    };
    match parse(&text, now_unix()) {
        Ok(schedule) => {
            if let Some(state) = app.compose.as_mut() {
                state.schedule = schedule;
            }
            app.schedule_edit = None;
            app.notice = Some(applied_notice(schedule));
        }
        Err(error) => app.notice = Some(format!("schedule: {error}")),
    }
}

fn applied_notice(schedule: Option<u64>) -> String {
    match schedule {
        Some(_) => format!("scheduled for {}", label(schedule)),
        None => "sending at the next drain".to_string(),
    }
}

/// Resolves a schedule input to an absolute send-after unix
/// time. Empty or "now" clears the schedule; a number with an
/// s/m/h/d suffix adds that delay to now; anything else is a
/// local datetime.
pub(super) fn parse(
    input: &str,
    now: u64,
) -> Result<Option<u64>, String> {
    let text = input.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("now") {
        return Ok(None);
    }
    if let Some(delay) = parse_relative(text) {
        return Ok(Some(now.saturating_add(delay)));
    }
    parse_absolute(text).map(Some)
}

fn parse_relative(text: &str) -> Option<u64> {
    let split = text.len().checked_sub(1)?;
    let (value, unit) = text.split_at(split);
    let count: u64 = value.trim().parse().ok()?;
    let seconds = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    };
    Some(count.saturating_mul(seconds))
}

fn parse_absolute(text: &str) -> Result<u64, String> {
    for format in DATE_FORMATS {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return local_to_unix(naive);
        }
    }
    Err(format!(
        "want a delay like 2h or a time like \
         2026-08-01 09:00, got {text:?}"
    ))
}

fn local_to_unix(naive: NaiveDateTime) -> Result<u64, String> {
    let Some(dt) = Local.from_local_datetime(&naive).single() else {
        return Err("ambiguous local time".to_string());
    };
    u64::try_from(dt.timestamp())
        .map_err(|_| "that time is in the past".to_string())
}

/// The review screen's Send line: "now" when unscheduled, else
/// the local send time.
pub(super) fn label(schedule: Option<u64>) -> String {
    let Some(at) = schedule else {
        return "now".to_string();
    };
    match Local.timestamp_opt(at as i64, 0).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => "scheduled".to_string(),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

pub(super) fn draw_modal(frame: &mut Frame, app: &App, area: Rect) {
    let Some(edit) = &app.schedule_edit else {
        return;
    };
    let theme = app.theme;
    let width = MODAL_WIDTH.min(area.width.saturating_sub(2));
    let height = 3u16.min(area.height);
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let block = Block::bordered()
        .title(" schedule ")
        .title_bottom(HINT)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            with_cursor(&edit.text, edit.cursor),
            Style::new().fg(theme.text_primary),
        ))),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_now_clear_the_schedule() {
        assert_eq!(parse("", 1_000), Ok(None));
        assert_eq!(parse("  now ", 1_000), Ok(None));
    }

    #[test]
    fn relative_delays_add_to_now() {
        assert_eq!(parse("30m", 1_000), Ok(Some(1_000 + 1_800)));
        assert_eq!(parse("2h", 1_000), Ok(Some(1_000 + 7_200)));
        assert_eq!(parse("3d", 0), Ok(Some(259_200)));
    }

    #[test]
    fn an_absolute_local_time_parses() {
        let at = parse("2026-08-01 09:00", 0).unwrap().unwrap();
        assert_eq!(label(Some(at)), "2026-08-01 09:00");
    }

    #[test]
    fn nonsense_is_rejected() {
        assert!(parse("soon", 0).is_err());
        assert!(parse("2026-13-40 99:99", 0).is_err());
    }
}
