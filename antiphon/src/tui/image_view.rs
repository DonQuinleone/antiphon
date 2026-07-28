use antiphon_core::Action;
use image::DynamicImage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

use super::app::{App, View};

const HEADER_ROWS: u16 = 1;
const HINT: &str = "esc/q close";

/// A full-pane image view: the decoded image and the graphics
/// protocol built for it at the size last drawn. The protocol
/// is rebuilt only when the canvas changes, so an unchanged
/// frame re-renders the cached encoding.
pub(super) struct ImageView {
    pub name: String,
    decoded: DynamicImage,
    protocol: Option<Protocol>,
    sized: Option<Rect>,
}

impl ImageView {
    fn new(name: String, decoded: DynamicImage) -> ImageView {
        ImageView {
            name,
            decoded,
            protocol: None,
            sized: None,
        }
    }
}

impl App {
    /// Enters the full-pane view for one image. The decode runs
    /// here, off the render path; a failure stays a notice and
    /// leaves the pager in place rather than opening an empty
    /// pane.
    pub(super) fn open_image_view(
        &mut self,
        name: String,
        bytes: &[u8],
    ) {
        match decode(bytes) {
            Ok(decoded) => {
                self.image_view = Some(ImageView::new(name, decoded));
                self.view = View::Image;
            }
            Err(error) => {
                self.notice = Some(format!("image {name}: {error}"));
            }
        }
    }

    pub(super) fn close_image_view(&mut self) {
        self.image_view = None;
        self.view = View::Pager;
    }

    pub(super) fn apply_in_image(&mut self, action: Action) {
        match action {
            Action::Back | Action::Quit => self.close_image_view(),
            _ => {}
        }
    }
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|error| error.to_string())
}

/// The protocol picker, detected once against the live
/// terminal; where no graphics protocol answers it degrades to
/// unicode half-blocks, which every terminal can draw.
pub(super) fn make_picker() -> Picker {
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}

/// Builds the encoded protocol for the current canvas if it is
/// missing or the canvas has resized. Returns an error string
/// for the caller to surface; never panics on an encode fault.
pub(super) fn prepare(
    view: &mut ImageView,
    picker: &mut Picker,
    terminal: Size,
) -> Option<String> {
    let area = Rect::new(0, 0, terminal.width, terminal.height);
    let canvas = canvas_area(area);
    if view.protocol.is_some() && view.sized == Some(canvas) {
        return None;
    }
    view.sized = Some(canvas);
    let size = Size::new(canvas.width, canvas.height);
    match picker.new_protocol(
        view.decoded.clone(),
        size,
        Resize::Fit(None),
    ) {
        Ok(protocol) => {
            view.protocol = Some(protocol);
            None
        }
        Err(error) => {
            view.protocol = None;
            Some(error.to_string())
        }
    }
}

pub(super) fn draw(frame: &mut Frame, app: &App, content: Rect) {
    let Some(view) = &app.image_view else {
        return;
    };
    let [header, canvas] = split(content);
    frame.render_widget(header_line(app, &view.name), header);
    match &view.protocol {
        Some(protocol) => {
            frame.render_widget(Image::new(protocol), canvas)
        }
        None => frame.render_widget(rendering(app), canvas),
    }
}

fn split(content: Rect) -> [Rect; 2] {
    Layout::vertical([
        Constraint::Length(HEADER_ROWS),
        Constraint::Min(0),
    ])
    .areas(content)
}

fn canvas_area(area: Rect) -> Rect {
    let (content, _status) = super::draw::split_status(area);
    split(content)[1]
}

fn header_line(app: &App, name: &str) -> Paragraph<'static> {
    let theme = app.theme;
    Paragraph::new(Line::from(vec![
        Span::styled(
            format!("image: {name}"),
            Style::new()
                .fg(theme.accent_strong)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {HINT}"),
            Style::new().fg(theme.text_muted),
        ),
    ]))
}

fn rendering(app: &App) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        "rendering...",
        Style::new().fg(app.theme.text_muted),
    )))
}

#[cfg(test)]
mod tests {
    use super::super::testkit::app_with_messages;
    use super::*;

    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::new_rgba8(1, 1);
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode a 1x1 png");
        bytes
    }

    #[test]
    fn a_decodable_image_opens_the_full_pane_view() {
        let mut app = app_with_messages(1);
        app.open_image_view("logo.png".to_string(), &tiny_png());
        assert_eq!(app.view, View::Image);
        let view = app.image_view.as_ref().expect("a view");
        assert_eq!(view.name, "logo.png");
        assert!(app.notice.is_none());
    }

    #[test]
    fn a_decode_failure_notices_and_stays_in_the_pager() {
        let mut app = app_with_messages(1);
        app.view = View::Pager;
        app.open_image_view("broken.png".to_string(), b"not an image");
        assert_eq!(app.view, View::Pager, "no empty pane opens");
        assert!(app.image_view.is_none());
        assert!(
            app.notice.as_deref().is_some_and(
                |notice| notice.starts_with("image broken")
            ),
            "{:?}",
            app.notice
        );
    }

    #[test]
    fn esc_closes_back_to_the_pager() {
        let mut app = app_with_messages(1);
        app.open_image_view("logo.png".to_string(), &tiny_png());
        app.apply_in_image(Action::Back);
        assert_eq!(app.view, View::Pager);
        assert!(app.image_view.is_none());
    }
}
