use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::actions::{account_of, folder_of};
use super::app::{App, View};
use super::sidebar::SidebarEntry;

/// The account root's display name; as a move target it is
/// the empty folder path.
const ROOT_LABEL: &str = "inbox";

const PICKER_WIDTH: u16 = 40;
const PICKER_MAX_ROWS: u16 = 16;
const BORDER_ROWS: u16 = 2;
const HINT: &str = " j/k move \u{b7} enter moves \u{b7} esc closes ";

/// The c picker: every folder of the message's account except
/// the one it already sits in, shown by alias where one is
/// configured, moving by real path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FolderPicker {
    pub folders: Vec<(String, String)>,
    pub selected: usize,
}

impl App {
    pub(super) fn open_folder_picker(&mut self) {
        let Some(message) = self.selected_message() else {
            return;
        };
        let account = account_of(&message.path);
        let current = folder_of(&message.path);
        let mut folders: Vec<(String, String)> = Vec::new();
        if !current.is_empty() {
            folders.push((String::new(), ROOT_LABEL.to_string()));
        }
        folders.extend(self.sidebar_entries.iter().filter_map(
            |entry| {
                let SidebarEntry::Folder {
                    account: entry_account,
                    name,
                    ..
                } = entry
                else {
                    return None;
                };
                let keep = *entry_account == account
                    && *name != current
                    && name != ROOT_LABEL;
                if !keep {
                    return None;
                }
                let label = self
                    .alias_for(&account, name)
                    .unwrap_or(name)
                    .to_string();
                Some((name.clone(), label))
            },
        ));
        if folders.is_empty() {
            self.notice =
                Some("no other folders on this account".to_string());
            return;
        }
        self.folder_picker = Some(FolderPicker {
            folders,
            selected: 0,
        });
    }
}

/// Keys while the picker is open; a chosen folder moves the
/// message and, from the pager, drops back to the list.
pub(super) fn feed(app: &mut App, key: KeyEvent) {
    let Some(picker) = app.folder_picker.as_mut() else {
        return;
    };
    let count = picker.folders.len();
    match key.code {
        KeyCode::Esc => app.folder_picker = None,
        KeyCode::Char('j') | KeyCode::Down => {
            picker.selected =
                (picker.selected + 1).min(count.saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            picker.selected = picker.selected.saturating_sub(1);
        }
        KeyCode::Enter => {
            let folder = picker.folders[picker.selected].0.clone();
            app.folder_picker = None;
            if app.view == View::Pager {
                app.view = View::List;
            }
            app.move_selected_to(&folder);
        }
        _ => {}
    }
}

pub(super) fn draw_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.folder_picker else {
        return;
    };
    let theme = app.theme;
    let width = PICKER_WIDTH.min(area.width.saturating_sub(2));
    let height = (picker.folders.len() as u16 + BORDER_ROWS)
        .min(PICKER_MAX_ROWS)
        .min(area.height.saturating_sub(2));
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let block = Block::bordered()
        .title(" move to ")
        .title_bottom(HINT)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let lines: Vec<Line<'static>> = picker
        .folders
        .iter()
        .enumerate()
        .map(|(index, (_, label))| {
            let style = if index == picker.selected {
                Style::new()
                    .fg(theme.accent_strong)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.text_primary)
            };
            Line::from(Span::styled(format!(" {label}"), style))
        })
        .collect();
    let visible = height.saturating_sub(BORDER_ROWS) as usize;
    let scroll = (picker.selected + 1).saturating_sub(visible) as u16;
    frame.render_widget(
        Paragraph::new(lines).scroll((scroll, 0)).block(block),
        modal,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyModifiers;

    use super::super::actions::OpIntent;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn picker_app() -> App {
        let mut app = app_with_messages(1);
        app.messages[0].path =
            std::path::PathBuf::from("store/maildir/work/cur/one.eml");
        app.sidebar_entries = vec![
            SidebarEntry::Folder {
                account: "work".to_string(),
                name: "inbox".to_string(),
                query: String::new(),
                unread: 0,
            },
            SidebarEntry::Folder {
                account: "work".to_string(),
                name: "inbox/accounts".to_string(),
                query: String::new(),
                unread: 0,
            },
            SidebarEntry::Folder {
                account: "home".to_string(),
                name: "archive".to_string(),
                query: String::new(),
                unread: 0,
            },
        ];
        app.folder_aliases = vec![(
            "work".to_string(),
            "inbox/accounts".to_string(),
            "accounts".to_string(),
        )];
        app
    }

    #[test]
    fn the_picker_lists_the_accounts_other_folders_by_alias() {
        let mut app = picker_app();
        app.open_folder_picker();
        let picker = app.folder_picker.as_ref().unwrap();
        assert_eq!(
            picker.folders,
            [("inbox/accounts".to_string(), "accounts".to_string())],
            "the root is no target while the message sits in it"
        );
    }

    #[test]
    fn entering_a_choice_moves_and_closes() {
        let mut app = picker_app();
        app.open_folder_picker();
        feed(&mut app, key(KeyCode::Enter));
        assert!(app.folder_picker.is_none());
        assert!(app.messages.is_empty());
        let Some(OpIntent::Move {
            to_folder,
            from_folder,
            ..
        }) = app.pending_ops.last()
        else {
            panic!("expected a move");
        };
        assert_eq!(to_folder, "inbox/accounts");
        assert_eq!(from_folder, "");
    }

    #[test]
    fn esc_closes_without_moving() {
        let mut app = picker_app();
        app.open_folder_picker();
        feed(&mut app, key(KeyCode::Esc));
        assert!(app.folder_picker.is_none());
        assert_eq!(app.messages.len(), 1);
    }
}
