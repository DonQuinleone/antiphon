use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::{App, DEFAULT_QUERY};
use super::commands::{Prompt, PromptKind};
use super::message_list::UNREAD_MARK;

pub(super) fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let line = match &app.prompt {
        Some(prompt) => prompt_line(theme, prompt),
        None => status_line(app),
    };
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(theme.surface)),
        area,
    );
}

fn prompt_line(theme: &Theme, prompt: &Prompt) -> Line<'static> {
    let sigil = match prompt.kind {
        PromptKind::Search => "/",
        PromptKind::Command => ":",
        PromptKind::AttachmentPath => "attach: ",
        PromptKind::SaveAttachment => "save to: ",
        PromptKind::ConfirmUnsubscribe => {
            return confirm_line(theme, &prompt.buffer);
        }
        PromptKind::ConfirmDraft => {
            return Line::from(Span::styled(
                "save as draft? y/n",
                Style::new().fg(theme.accent_strong),
            ));
        }
    };
    Line::from(vec![
        Span::styled(
            sigil.to_string(),
            Style::new().fg(theme.accent_strong),
        ),
        Span::styled(
            prompt.buffer.clone(),
            Style::new().fg(theme.text_primary),
        ),
        Span::styled(
            "\u{258c}".to_string(),
            Style::new().fg(theme.accent),
        ),
    ])
}

fn confirm_line(theme: &Theme, list: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("unsubscribe from {list}? "),
            Style::new().fg(theme.text_primary),
        ),
        Span::styled(
            "y/n".to_string(),
            Style::new().fg(theme.accent_strong),
        ),
    ])
}

fn status_line(app: &App) -> Line<'static> {
    let theme = app.theme;
    if let Some(state) = &app.compose {
        return compose_status(theme, state);
    }
    let text = match &app.notice {
        Some(notice) => notice.clone(),
        None => {
            format!(
                "{}{} of {} messages \u{b7} {} unread shown \u{b7} \
             theme {}{}",
                context_prefix(app),
                app.messages.len(),
                app.total_messages,
                app.unread_count(),
                app.theme.name,
                queued_suffix(app.pending_ops.len()),
            ) + &sync_suffix(app)
        }
    };
    Line::from(vec![
        Span::styled(UNREAD_MARK, Style::new().fg(theme.accent)),
        Span::styled(text, Style::new().fg(theme.text_muted)),
    ])
}

fn compose_status(
    theme: &Theme,
    state: &super::compose::ComposeState,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!(
            "compose \u{b7} {} \u{b7} ctrl-h headers \u{b7} \
             :q in the editor reviews",
            state.account()
        ),
        Style::new().fg(theme.text_muted),
    )];
    if let Some(label) = state.plan().label() {
        spans.push(Span::styled(
            format!(" {label}"),
            Style::new().fg(theme.accent),
        ));
    }
    Line::from(spans)
}

fn context_prefix(app: &App) -> String {
    let scope = app.scope.label();
    match &app.active_search {
        Some(name) => format!("{scope} \u{b7} {name} \u{b7} "),
        None => {
            format!(
                "{scope} \u{b7} {}",
                query_prefix(&app.current_query)
            )
        }
    }
}

fn query_prefix(query: &str) -> String {
    if query == DEFAULT_QUERY {
        return String::new();
    }
    format!("{query} \u{b7} ")
}

fn queued_suffix(pending: usize) -> String {
    if pending == 0 {
        return String::new();
    }
    format!(" \u{b7} {pending} queued for antiphond")
}

fn sync_suffix(app: &App) -> String {
    use antiphon_sync::SyncState;

    let Some(progress) = &app.sync_progress else {
        return String::new();
    };
    if progress.state != SyncState::Syncing {
        return String::new();
    }
    format!(
        " \u{b7} syncing {}/{} {}/{}",
        progress.account,
        progress.folder,
        progress.fetched,
        progress.total,
    )
}
