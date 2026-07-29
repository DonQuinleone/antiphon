//! Bulk actions over a whole search result: trash, archive,
//! move or permanently delete every message the current query
//! matches, not just the loaded window. A `:`-command arms an
//! action, an unlimited fetch expands it into one op per
//! message, and a confirm modal states the exact count and
//! shows a sample of subjects before any op is queued.

use std::collections::HashSet;

use antiphon_store::{MessageSummary, SearchIndex, StoreLayout};
use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::actions::{OpIntent, account_of, folder_of};
use super::app::App;
use super::commands::PromptKind;

/// Subjects listed in the confirm modal as a sample of what
/// the action will touch.
const EXAMPLE_LIMIT: usize = 10;
/// Above this many matches the modal warns more loudly, since
/// the action sweeps well beyond the visible window.
const BULK_WARN_THRESHOLD: usize = 100;
const NO_SUBJECT: &str = "(no subject)";

const MODAL_WIDTH: u16 = 60;
const BORDER_ROWS: u16 = 2;
const HINT: &str = " y confirm \u{b7} esc/n cancel ";

/// The four bulk commands, each applied over the current query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BulkAction {
    Trash,
    Archive,
    Move(String),
    Delete,
}

/// The command arms an action; the unlimited fetch turns it
/// into a pending confirmation carrying the ops to queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Bulk {
    Armed(BulkAction),
    Confirm(BulkConfirm),
}

/// What the modal shows and what confirming queues; the ops
/// are built up front so the confirm step only pushes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BulkConfirm {
    pub action: BulkAction,
    pub count: usize,
    pub examples: Vec<String>,
    pub intents: Vec<OpIntent>,
}

type BulkParse = fn(&str) -> Option<BulkAction>;

/// name, usage, and the parser turning its argument into an
/// action; the argless commands ignore the argument.
const BULK_COMMANDS: [(&str, &str, BulkParse); 4] = [
    ("trash-all", "trash-all", |_| Some(BulkAction::Trash)),
    ("archive-all", "archive-all", |_| Some(BulkAction::Archive)),
    ("delete-all", "delete-all", |_| Some(BulkAction::Delete)),
    ("move-all", "move-all <folder>", parse_move),
];

fn parse_move(argument: &str) -> Option<BulkAction> {
    let folder = argument.trim();
    (!folder.is_empty()).then(|| BulkAction::Move(folder.to_string()))
}

impl BulkAction {
    fn verb(&self) -> &'static str {
        match self {
            BulkAction::Trash => "trash",
            BulkAction::Archive => "archive",
            BulkAction::Move(_) => "move",
            BulkAction::Delete => "permanent delete",
        }
    }

    fn title(&self, count: usize) -> String {
        match self {
            BulkAction::Trash => format!(" trash {count} messages "),
            BulkAction::Archive => {
                format!(" archive {count} messages ")
            }
            BulkAction::Move(folder) => {
                format!(" move {count} messages to {folder} ")
            }
            BulkAction::Delete => {
                format!(" delete {count} messages permanently ")
            }
        }
    }

    fn is_destructive(&self) -> bool {
        matches!(self, BulkAction::Delete)
    }
}

/// Recognises a bulk command and arms it for `run_pending`;
/// `false` leaves the command for the ordinary dispatch.
pub(super) fn arm(app: &mut App, command: &str) -> bool {
    for (name, usage, parse) in BULK_COMMANDS {
        let Some(argument) =
            super::commands::argument_of(command, name)
        else {
            continue;
        };
        match parse(argument) {
            Some(action) => app.bulk = Some(Bulk::Armed(action)),
            None => app.notice = Some(format!("usage: {usage}")),
        }
        return true;
    }
    false
}

/// Expands an armed action over the FULL matching set (no
/// window limit), then opens the confirm modal; run from the
/// command prompt where the store layout is in hand.
pub(super) fn run_pending(app: &mut App, layout: &StoreLayout) {
    let Some(bulk) = app.bulk.take() else {
        return;
    };
    let Bulk::Armed(action) = bulk else {
        app.bulk = Some(bulk);
        return;
    };
    match matching_summaries(app, layout) {
        Ok(summaries) => open_confirm(app, action, summaries),
        Err(error) => app.notice = Some(error),
    }
}

/// Every message the current query matches, scoped exactly as
/// the list is but with no window limit, so the action covers
/// all matches rather than the loaded rows alone.
fn matching_summaries(
    app: &App,
    layout: &StoreLayout,
) -> Result<Vec<MessageSummary>, String> {
    let query = app
        .scoped(&app.current_query)
        .map_err(|error| error.to_string())?;
    let index =
        SearchIndex::open(layout).map_err(|error| error.to_string())?;
    index.query(&query, None).map_err(|error| error.to_string())
}

pub(super) fn open_confirm(
    app: &mut App,
    action: BulkAction,
    summaries: Vec<MessageSummary>,
) {
    if summaries.is_empty() {
        app.notice = Some("nothing matches this search".to_string());
        return;
    }
    let intents = intents_for(app, &action, &summaries);
    app.bulk = Some(Bulk::Confirm(BulkConfirm {
        action,
        count: summaries.len(),
        examples: examples_of(&summaries),
        intents,
    }));
    app.open_prompt(PromptKind::ConfirmBulk);
}

/// Queues the confirmed ops and nudges the daemon to replay
/// them; the rows leave the view at once, the daemon reconciles
/// the index later.
pub(super) fn confirm(app: &mut App) {
    if queue_confirmed(app) == 0 {
        return;
    }
    super::nudge_daemon();
}

pub(super) fn queue_confirmed(app: &mut App) -> usize {
    let Some(Bulk::Confirm(confirm)) = app.bulk.take() else {
        return 0;
    };
    let count = confirm.count;
    let touched: HashSet<String> = confirm
        .intents
        .iter()
        .map(|op| op_message_id(op).to_string())
        .collect();
    app.pending_ops.extend(confirm.intents);
    app.messages
        .retain(|message| !touched.contains(&message.id));
    app.total_messages =
        app.total_messages.saturating_sub(count as u32);
    app.selected =
        app.selected.min(app.messages.len().saturating_sub(1));
    app.notice = Some(format!(
        "queued {} for {count} messages",
        confirm.action.verb(),
    ));
    count
}

pub(super) fn cancel(app: &mut App) {
    app.bulk = None;
    app.notice = Some("cancelled; nothing queued".to_string());
}

fn op_message_id(op: &OpIntent) -> &str {
    match op {
        OpIntent::Move { message_id, .. }
        | OpIntent::Delete { message_id, .. }
        | OpIntent::Flag { message_id, .. } => message_id,
    }
}

/// One op per message, each routed by the copy in the account
/// it was indexed under (account and source folder from the
/// summary's own path), so a message synced to two accounts is
/// handled per account rather than by a single raw path.
fn intents_for(
    app: &App,
    action: &BulkAction,
    summaries: &[MessageSummary],
) -> Vec<OpIntent> {
    summaries
        .iter()
        .map(|summary| intent_for(app, action, summary))
        .collect()
}

fn intent_for(
    app: &App,
    action: &BulkAction,
    summary: &MessageSummary,
) -> OpIntent {
    // The bulk set is fetched straight from the index, so its
    // paths skip the list's per-scope re-pointing; choose the
    // copy in the account being viewed here too, or a message
    // Bcc'd to a second account would be acted on in the wrong
    // one.
    let path =
        super::mailpaths::scoped_path(&app.scope, &app.accounts, summary);
    let account = account_of(&path);
    match action {
        BulkAction::Delete => OpIntent::Delete {
            account,
            message_id: summary.id.clone(),
        },
        BulkAction::Trash => {
            let folder = app.trash_folder_of(&account);
            move_intent(summary, &path, account, folder)
        }
        BulkAction::Archive => {
            let folder = app.archive_folder_of(&account);
            move_intent(summary, &path, account, folder)
        }
        BulkAction::Move(input) => {
            let folder = app.resolve_folder(&account, input);
            move_intent(summary, &path, account, folder)
        }
    }
}

fn move_intent(
    summary: &MessageSummary,
    path: &std::path::Path,
    account: String,
    to_folder: String,
) -> OpIntent {
    OpIntent::Move {
        account,
        message_id: summary.id.clone(),
        to_folder,
        from_folder: folder_of(path),
    }
}

fn examples_of(summaries: &[MessageSummary]) -> Vec<String> {
    summaries
        .iter()
        .take(EXAMPLE_LIMIT)
        .map(|summary| subject_or_placeholder(&summary.subject))
        .collect()
}

fn subject_or_placeholder(subject: &str) -> String {
    match subject.trim().is_empty() {
        true => NO_SUBJECT.to_string(),
        false => subject.trim().to_string(),
    }
}

pub(super) fn draw_modal(frame: &mut Frame, app: &App, area: Rect) {
    let Some(Bulk::Confirm(confirm)) = &app.bulk else {
        return;
    };
    let theme = app.theme;
    let lines = modal_lines(theme, confirm);
    let width = MODAL_WIDTH.min(area.width.saturating_sub(2));
    let height = (lines.len() as u16 + BORDER_ROWS)
        .min(area.height.saturating_sub(2));
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let border = match confirm.action.is_destructive() {
        true => theme.accent_strong,
        false => theme.accent,
    };
    let block = Block::bordered()
        .title(confirm.action.title(confirm.count))
        .title_bottom(HINT)
        .border_style(Style::new().fg(border))
        .style(Style::new().bg(theme.surface));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        modal,
    );
}

fn modal_lines(
    theme: &Theme,
    confirm: &BulkConfirm,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(warning) = warning_line(&confirm.action, confirm.count)
    {
        lines.push(Line::from(Span::styled(
            warning,
            Style::new()
                .fg(theme.accent_strong)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());
    }
    for subject in &confirm.examples {
        lines.push(Line::from(Span::styled(
            format!(" {subject}"),
            Style::new().fg(theme.text_primary),
        )));
    }
    if confirm.count > confirm.examples.len() {
        let more = confirm.count - confirm.examples.len();
        lines.push(Line::from(Span::styled(
            format!(" and {more} more"),
            Style::new().fg(theme.text_muted),
        )));
    }
    lines
}

fn warning_line(action: &BulkAction, count: usize) -> Option<String> {
    if action.is_destructive() {
        return Some(format!(
            "Permanently deletes {count} messages. \
             This cannot be undone.",
        ));
    }
    if count > BULK_WARN_THRESHOLD {
        return Some(format!(
            "This affects {count} messages, far beyond the \
             visible list.",
        ));
    }
    None
}
#[cfg(test)]
#[path = "bulk_tests.rs"]
mod tests;
