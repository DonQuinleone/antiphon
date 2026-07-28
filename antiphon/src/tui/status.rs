use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::{App, DEFAULT_QUERY, View};
use super::commands::{Prompt, PromptKind};
use super::message_list::UNREAD_MARK;
use super::settings::SettingsTab;

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
        PromptKind::ConfirmDelete => {
            return Line::from(Span::styled(
                "delete forever? this cannot be undone \u{b7} y/n",
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
    if app.view == View::Settings {
        return settings_status(app, theme);
    }
    if let Some(state) = &app.compose {
        return compose_status(app, theme, state);
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

/// The settings keys, same shape as the compose hint: one line
/// that follows whichever tab is open, or the delete
/// confirmation while one is pending.
fn settings_status(app: &App, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        settings_hint(app),
        Style::new().fg(theme.text_muted),
    ))
}

fn settings_hint(app: &App) -> String {
    let Some(state) = &app.settings else {
        return String::new();
    };
    if let Some(flow) = &app.oauth_flow {
        return flow.status.clone();
    }
    if app.folder_alias_edit.is_some() {
        return "type the alias \u{b7} enter saves \u{b7} \
             esc cancels"
            .to_string();
    }
    if state.pending_delete.is_some() {
        return "y confirm \u{b7} any other key cancels".to_string();
    }
    match state.tab {
        SettingsTab::Accounts => "j/k select \u{b7} a add \u{b7} \
             e edit \u{b7} o sign in \u{b7} d delete \u{b7} \
             tab essentials \u{b7} esc back"
            .to_string(),
        SettingsTab::Essentials => "j/k select \u{b7} h/l change \
             \u{b7} tab folders \u{b7} esc back"
            .to_string(),
        SettingsTab::Folders => "j/k select \u{b7} enter edits \
             alias \u{b7} tab accounts \u{b7} esc back"
            .to_string(),
    }
}

/// The compose keys live here rather than in body rows, so
/// every stage keeps the same shape and the hint follows the
/// stage: fields, editor, review, or an open completion.
fn compose_status(
    app: &App,
    theme: &Theme,
    state: &super::compose::ComposeState,
) -> Line<'static> {
    let hint = compose_hint(app, state);
    let mut spans = vec![Span::styled(
        format!("{} \u{b7} {hint}", state.account()),
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

fn compose_hint(
    app: &App,
    state: &super::compose::ComposeState,
) -> String {
    use super::app::View;

    if state.completion.is_some() && app.view == View::Compose {
        return "tab completes \u{b7} ctrl-n/p select \u{b7} \
                esc dismisses"
            .to_string();
    }
    match app.view {
        View::Editor => ":q reviews \u{b7} ctrl-e headers".to_string(),
        View::Review => "y send \u{b7} q draft \u{b7} e body \
                         \u{b7} h headers \u{b7} a attach \u{b7} \
                         s/x seal \u{b7} ? keys"
            .to_string(),
        _ => "tab/shift-tab fields \u{b7} ctrl-e editor \u{b7} \
              esc backs out"
            .to_string(),
    }
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
