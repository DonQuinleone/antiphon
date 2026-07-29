use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::{App, DEFAULT_QUERY, View};
use super::commands::{Prompt, PromptKind};
use super::message_list::UNREAD_MARK;
use crate::tui::settings::SettingsTab;

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
        PromptKind::ConfirmBulk => {
            return Line::from(Span::styled(
                "apply to the whole search? \u{b7} y confirm \
                 \u{b7} esc/n cancel",
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
    if let Some(notice) = &app.notice {
        return Line::from(vec![
            Span::styled(UNREAD_MARK, Style::new().fg(theme.accent)),
            Span::styled(
                notice.clone(),
                Style::new().fg(theme.text_muted),
            ),
        ]);
    }
    let detail = format!(
        "{}{} of {} messages \u{b7} {} unread shown \u{b7} \
         theme {}{}",
        context_detail(app),
        app.messages.len(),
        app.total_messages,
        app.unread_count(),
        app.theme.name,
        queued_suffix(app.pending_ops.len()),
    ) + &sync_suffix(app)
        + &auth_suffix(app);
    Line::from(vec![
        Span::styled(UNREAD_MARK, Style::new().fg(theme.accent)),
        Span::styled(
            app.scope.label().to_string(),
            Style::new()
                .fg(theme.accent_strong)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(detail, Style::new().fg(theme.text_muted)),
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
    if let Some(flow) = &app.oauth_flow {
        return flow.status.clone();
    }
    let Some(state) = &app.settings else {
        return String::new();
    };
    if app.folder_alias_edit.is_some() {
        return "type the alias \u{b7} enter saves \u{b7} \
             esc cancels"
            .to_string();
    }
    if state.pending_delete.is_some() || state.pending_revoke.is_some()
    {
        return "y confirm \u{b7} any other key cancels".to_string();
    }
    match state.tab {
        SettingsTab::Accounts => "j/k select \u{b7} J/K reorder \
             \u{b7} a add \u{b7} e edit \u{b7} o sign in \u{b7} \
             x revoke \u{b7} d delete \u{b7} tab essentials \u{b7} \
             esc back"
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
    if state.read_receipt {
        spans.push(Span::styled(
            " [receipt]".to_string(),
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
        View::Review => "y send \u{b7} @ schedule \u{b7} q draft \
                         \u{b7} e body \u{b7} h headers \u{b7} a \
                         attach \u{b7} s/x seal \u{b7} k receipt \
                         \u{b7} ? keys"
            .to_string(),
        _ => "tab/shift-tab fields \u{b7} ctrl-e editor \u{b7} \
              esc backs out"
            .to_string(),
    }
}

/// The scope name is its own span so it reads as the active
/// account indicator; this is what trails it: the active saved
/// search or a non-default query, otherwise just the separator.
fn context_detail(app: &App) -> String {
    match &app.active_search {
        Some(name) => format!(" \u{b7} {name} \u{b7} "),
        None => {
            format!(" \u{b7} {}", query_prefix(&app.current_query))
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

/// Accounts the daemon reports as needing a fresh OAuth
/// sign-in surface here too, so the problem is visible
/// without opening settings.
fn auth_suffix(app: &App) -> String {
    if app.auth_failures.is_empty() {
        return String::new();
    }
    let verb = match app.auth_failures.len() {
        1 => "needs",
        _ => "need",
    };
    format!(
        " \u{b7} auth: {} {verb} sign-in",
        app.auth_failures.join(", ")
    )
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

#[cfg(test)]
mod tests {
    use super::super::scope::ViewScope;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect()
    }

    #[test]
    fn auth_failures_surface_in_the_status_line() {
        let mut app = app_with_messages(1);
        assert!(
            !line_text(&status_line(&app)).contains("auth:"),
            "quiet while every account is signed in"
        );
        app.auth_failures = vec!["work".to_string()];
        let text = line_text(&status_line(&app));
        assert!(text.contains("auth: work needs sign-in"), "{text}");
        app.auth_failures.push("personal".to_string());
        let text = line_text(&status_line(&app));
        assert!(
            text.contains("auth: work, personal need sign-in"),
            "{text}"
        );
    }

    #[test]
    fn a_notice_still_wins_over_the_auth_segment() {
        let mut app = app_with_messages(1);
        app.auth_failures = vec!["work".to_string()];
        app.notice = Some("saved".to_string());
        let text = line_text(&status_line(&app));
        assert!(text.contains("saved"));
        assert!(!text.contains("auth:"), "{text}");
    }

    #[test]
    fn the_scope_names_the_active_account_as_its_own_span() {
        let mut app = app_with_messages(1);
        app.scope = ViewScope::Account("work".into());
        let line = status_line(&app);
        let scope = &line.spans[1];
        assert_eq!(scope.content, "work");
        assert!(
            scope.style.add_modifier.contains(Modifier::BOLD),
            "the sole indicator stands out from the muted detail"
        );

        app.scope = ViewScope::Unified;
        assert_eq!(status_line(&app).spans[1].content, "unified");
    }

    #[test]
    fn the_accounts_footer_documents_the_reorder_keys() {
        let mut app = app_with_messages(1);
        app.view = View::Settings;
        app.open_settings();
        let text = line_text(&status_line(&app));
        assert!(
            text.contains("j/k select") && text.contains("J/K reorder"),
            "the footer names both selection and reorder: {text}"
        );
    }

    #[test]
    fn a_running_sign_in_owns_the_settings_hint() {
        let mut app = app_with_messages(1);
        app.view = View::Settings;
        app.oauth_flow =
            Some(super::super::oauthflow::test_flow("work"));
        let text = line_text(&status_line(&app));
        assert!(text.contains("waiting for work"), "{text}");
    }
}
