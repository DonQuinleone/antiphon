use antiphon_core::Action;

use super::app::{App, View};
use super::link_picker::LinkPicker;

const PAGER_SCROLL_ROWS: u16 = 1;
const PAGER_HALF_PAGE_ROWS: u16 = 10;

impl App {
    pub(super) fn apply_in_pager(&mut self, action: Action) {
        match action {
            Action::MoveDown | Action::Open => {
                self.scroll_pager(PAGER_SCROLL_ROWS as i32)
            }
            Action::MoveUp => {
                self.scroll_pager(-(PAGER_SCROLL_ROWS as i32))
            }
            Action::HalfPageDown => {
                self.scroll_pager(PAGER_HALF_PAGE_ROWS as i32)
            }
            Action::HalfPageUp => {
                self.scroll_pager(-(PAGER_HALF_PAGE_ROWS as i32))
            }
            Action::Top => self.pager_scroll = 0,
            Action::Bottom => {
                self.pager_scroll =
                    self.pager_line_count().clamp(0, u16::MAX as i32)
                        as u16
            }
            Action::ToggleHeaders => {
                self.headers_all = !self.headers_all
            }
            Action::OpenLink => self.open_link_picker(),
            Action::Attachments => self.toggle_drawer(),
            Action::Archive => {
                self.view = View::List;
                self.archive_selected();
            }
            Action::MoveTo => self.open_folder_picker(),
            Action::Back | Action::Quit => self.view = View::List,
            _ => self.not_built_notice(),
        }
    }

    fn toggle_drawer(&mut self) {
        if self.pager_attachments.is_empty() {
            self.notice =
                Some("no attachments on this message".to_string());
            return;
        }
        self.drawer_open = !self.drawer_open;
    }

    fn open_link_picker(&mut self) {
        if self.pager_rendered.links.is_empty() {
            self.notice = Some("no links in this message".to_string());
            return;
        }
        self.link_picker = Some(LinkPicker::default());
    }

    fn scroll_pager(&mut self, rows: i32) {
        let scrolled = i32::from(self.pager_scroll) + rows;
        let ceiling = self.pager_line_count();
        self.pager_scroll = scrolled.clamp(0, ceiling) as u16;
    }

    fn pager_line_count(&self) -> i32 {
        self.pager_body.lines().count() as i32
    }
}
