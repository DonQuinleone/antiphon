use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::app::App;
use super::cells::{UNREAD_MARK, date_spans, format_date, sender_name};
use super::thread_tree::{ThreadNode, ThreadTree};

const INDENT: &str = "  ";
const DATE_GAP: &str = "  ";
const EXPANDED_MARK: &str = "\u{25be} ";
const COLLAPSED_MARK: &str = "\u{25b8} ";
const LEAF_MARK: &str = "\u{00b7} ";
const OWN_MAIL_PREFIX: &str = "\u{2192} ";
const NO_SUBJECT: &str = "(no subject)";

/// Draws the thread as an indented, foldable tree in place of
/// the flat list. The selected message keeps its highlight and
/// still feeds the reading pane.
pub(super) fn draw_thread(frame: &mut Frame, app: &App, area: Rect) {
    let Some(tree) = &app.thread_tree else {
        return;
    };
    let theme = app.theme;
    let [head, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
            .areas(area);
    frame.render_widget(heading(app, tree), head);
    let visible = tree.visible();
    let date_cols = date_width(app, &visible);
    let items: Vec<ListItem> = visible
        .iter()
        .map(|position| row(app, tree, *position, date_cols))
        .collect();
    let selected = visible
        .iter()
        .position(|position| *position == app.selected);
    let list = List::new(items).highlight_style(
        Style::new().bg(theme.selection_bg).fg(theme.selection_fg),
    );
    let mut state = ListState::default();
    state.select(selected);
    frame.render_stateful_widget(list, body, &mut state);
}

fn heading(app: &App, tree: &ThreadTree) -> Paragraph<'static> {
    let count = tree.nodes.len();
    let text = format!(
        "thread \u{00b7} {count} message{} \u{00b7} \
         za fold \u{00b7} esc back",
        if count == 1 { "" } else { "s" },
    );
    Paragraph::new(Line::from(Span::styled(
        text,
        Style::new()
            .fg(app.theme.text_muted)
            .add_modifier(Modifier::BOLD),
    )))
}

/// The date column is as wide as the widest rendered date, so
/// the reply tree begins at the same column on every row.
fn date_width(app: &App, visible: &[usize]) -> usize {
    visible
        .iter()
        .map(|position| {
            format_date(
                app.messages[*position].date_unix,
                &app.date_format,
            )
            .chars()
            .count()
        })
        .max()
        .unwrap_or(0)
}

fn row(
    app: &App,
    tree: &ThreadTree,
    position: usize,
    date_cols: usize,
) -> ListItem<'static> {
    let theme = app.theme;
    let message = &app.messages[position];
    let node = &tree.nodes[position];
    let rendered = format_date(message.date_unix, &app.date_format);
    let pad = date_cols.saturating_sub(rendered.chars().count());
    let mut spans = date_spans(theme, rendered);
    spans.push(Span::raw(format!("{:1$}{DATE_GAP}", "", pad)));
    let gutter =
        format!("{}{}", INDENT.repeat(node.depth), marker(node));
    spans.push(Span::styled(gutter, Style::new().fg(theme.text_muted)));
    if message.unread {
        spans.push(Span::styled(
            UNREAD_MARK,
            Style::new().fg(theme.unread_marker),
        ));
    }
    spans.push(Span::styled(
        format!("{}  ", who(app, message)),
        weight(Style::new().fg(theme.list_from), message.unread),
    ));
    let subject = if message.subject.is_empty() {
        NO_SUBJECT
    } else {
        &message.subject
    };
    spans.push(Span::styled(
        subject.to_string(),
        weight(Style::new().fg(theme.list_subject), message.unread),
    ));
    ListItem::new(Line::from(spans))
}

/// A folded node wears the count it hides; an open node with
/// replies wears an open marker; a leaf just a dot.
fn marker(node: &ThreadNode) -> String {
    if node.descendants == 0 {
        return LEAF_MARK.to_string();
    }
    if node.collapsed {
        return format!("{COLLAPSED_MARK}(+{}) ", node.descendants);
    }
    EXPANDED_MARK.to_string()
}

fn who(app: &App, message: &antiphon_store::MessageSummary) -> String {
    if app.is_own(&message.from) {
        return format!(
            "{OWN_MAIL_PREFIX}{}",
            sender_name(&message.to)
        );
    }
    sender_name(&message.from)
}

fn weight(style: Style, unread: bool) -> Style {
    if unread {
        return style.add_modifier(Modifier::BOLD);
    }
    style
}

#[cfg(test)]
mod tests {
    use antiphon_store::MessageSummary;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::super::testkit::app_with_messages;
    use super::*;

    const ISO: &str = "%Y-%m-%d %H:%M";
    const ROOT_UNIX: i64 = 1_768_000_000;
    const REPLY_UNIX: i64 = 1_768_086_400;

    fn summary(
        id: &str,
        from: &str,
        to: &str,
        parent: Option<&str>,
        date: i64,
    ) -> MessageSummary {
        MessageSummary {
            id: id.to_string(),
            thread_id: "t1".to_string(),
            subject: format!("subject of {id}"),
            from: from.to_string(),
            to: to.to_string(),
            date_unix: date,
            tags: Vec::new(),
            unread: false,
            path: std::path::PathBuf::new(),
            in_reply_to: parent.map(str::to_string),
            references: parent.into_iter().map(str::to_string).collect(),
        }
    }

    fn threaded_app() -> App {
        let mut app = app_with_messages(0);
        app.date_format = ISO.to_string();
        app.own_addresses = vec!["me@example.com".to_string()];
        let messages = vec![
            summary(
                "root",
                "Alice <alice@example.com>",
                "me@example.com",
                None,
                ROOT_UNIX,
            ),
            summary(
                "reply",
                "Me <me@example.com>",
                "alice@example.com",
                Some("root"),
                REPLY_UNIX,
            ),
        ];
        app.set_results(messages, 2, "thread:t1".to_string());
        app
    }

    fn render(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_thread(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
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

    #[test]
    fn every_thread_row_shows_its_own_date() {
        let app = threaded_app();
        let buffer = render(&app, 80, 6);
        let root_date = format_date(ROOT_UNIX, ISO);
        let reply_date = format_date(REPLY_UNIX, ISO);
        assert_ne!(root_date, reply_date);
        assert!(
            row_text(&buffer, 1).contains(&root_date),
            "root row missing its date"
        );
        assert!(
            row_text(&buffer, 2).contains(&reply_date),
            "reply row missing its date"
        );
    }

    #[test]
    fn the_date_column_aligns_the_reply_tree() {
        let app = threaded_app();
        let buffer = render(&app, 80, 6);
        let root = row_text(&buffer, 1);
        let reply = row_text(&buffer, 2);
        let expanded = EXPANDED_MARK.chars().next().unwrap();
        let leaf = LEAF_MARK.chars().next().unwrap();
        let root_mark = root.find(expanded).unwrap();
        let reply_mark = reply.find(leaf).unwrap();
        assert_eq!(
            reply_mark,
            root_mark + INDENT.chars().count(),
            "the reply nests one level under the root: \
             {root:?} {reply:?}"
        );
    }

    #[test]
    fn an_own_reply_reads_by_its_recipient() {
        let app = threaded_app();
        let buffer = render(&app, 80, 6);
        let reply = row_text(&buffer, 2);
        assert!(
            reply.contains(OWN_MAIL_PREFIX.trim_end()),
            "own reply lacks the sent marker: {reply:?}"
        );
        assert!(
            reply.contains("alice"),
            "own reply should name its recipient: {reply:?}"
        );
    }
}
