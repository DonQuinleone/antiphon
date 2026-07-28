use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::app::App;
use super::cells::{UNREAD_MARK, sender_name};
use super::thread_tree::{ThreadNode, ThreadTree};

const INDENT: &str = "  ";
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
    let items: Vec<ListItem> = visible
        .iter()
        .map(|position| row(app, tree, *position))
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

fn row(
    app: &App,
    tree: &ThreadTree,
    position: usize,
) -> ListItem<'static> {
    let theme = app.theme;
    let message = &app.messages[position];
    let node = &tree.nodes[position];
    let gutter =
        format!("{}{}", INDENT.repeat(node.depth), marker(node));
    let mut spans =
        vec![Span::styled(gutter, Style::new().fg(theme.text_muted))];
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
