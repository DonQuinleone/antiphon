//! A segmented toggle: every option of a small enumerated
//! field drawn inline on one row, the selected one carrying a
//! reversed highlight, like a segmented control. The caller
//! prepends its own label; here only the options are rendered,
//! so the same widget serves the account form and settings.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// The colours a segmented row paints with: the selected
/// option reversed onto `selected_bg` in `selected_fg`, the
/// rest drawn flat in `unselected_fg`.
#[derive(Clone, Copy)]
pub(in crate::tui) struct SegmentStyle {
    pub(in crate::tui) selected_bg: Color,
    pub(in crate::tui) selected_fg: Color,
    pub(in crate::tui) unselected_fg: Color,
}

const GAP: &str = " ";

/// One span per option, each padded with a space either side
/// so its highlight reads as a pill, and a gap span between
/// neighbours. The option at `selected` carries the reversed
/// highlight; the rest are flat.
pub(in crate::tui) fn segments(
    options: &[&str],
    selected: usize,
    style: SegmentStyle,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(options.len() * 2);
    for (index, option) in options.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(GAP));
        }
        spans.push(segment(option, index == selected, style));
    }
    spans
}

fn segment(
    label: &str,
    selected: bool,
    style: SegmentStyle,
) -> Span<'static> {
    let text = format!(" {label} ");
    if selected {
        return Span::styled(
            text,
            Style::new()
                .bg(style.selected_bg)
                .fg(style.selected_fg)
                .add_modifier(Modifier::BOLD),
        );
    }
    Span::styled(text, Style::new().fg(style.unselected_fg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> SegmentStyle {
        SegmentStyle {
            selected_bg: Color::Rgb(0x10, 0x20, 0x30),
            selected_fg: Color::Rgb(0xf0, 0xf0, 0xf0),
            unselected_fg: Color::Rgb(0x60, 0x60, 0x60),
        }
    }

    #[test]
    fn every_option_is_rendered_in_order() {
        let options = ["off", "on", "auto"];
        let spans = segments(&options, 1, style());
        let joined: String =
            spans.iter().map(|span| span.content.as_ref()).collect();
        for option in options {
            assert!(joined.contains(option), "{option} missing");
        }
        assert!(
            joined.find("off").unwrap() < joined.find("auto").unwrap(),
            "options keep their order: {joined:?}"
        );
    }

    #[test]
    fn the_selected_option_carries_the_highlight() {
        let spans = segments(&["off", "on"], 1, style());
        let selected = spans
            .iter()
            .find(|span| span.content.contains("on"))
            .expect("the on span");
        assert_eq!(selected.style.bg, Some(style().selected_bg));
        assert_eq!(selected.style.fg, Some(style().selected_fg));
        assert!(selected.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unselected_options_are_flat_with_no_background() {
        let spans = segments(&["off", "on"], 1, style());
        let flat = spans
            .iter()
            .find(|span| span.content.contains("off"))
            .expect("the off span");
        assert_eq!(flat.style.bg, None);
        assert_eq!(flat.style.fg, Some(style().unselected_fg));
        assert!(!flat.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_single_option_renders_without_a_leading_gap() {
        let spans = segments(&["only"], 0, style());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), " only ");
    }
}
