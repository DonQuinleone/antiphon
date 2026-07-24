mod gallery;

use ratatui::style::Color;

pub const fn hex(rgb: u32) -> Color {
    assert!(rgb <= 0xff_ffff, "hex colour out of 24-bit range");
    Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub surface: Color,
    pub border: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_strong: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub unread_marker: Color,
    pub count: Color,
    pub status_ok: Color,
    pub status_error: Color,
    pub diff_add: Color,
    pub diff_remove: Color,
}

const DEPTH: Color = hex(0x0c0f1d);
const SURFACE: Color = hex(0x141a2e);
const RAISED: Color = hex(0x1e2742);
const LINES: Color = hex(0x2c3860);
const SLATE: Color = hex(0x7a86ad);
const PARCHMENT: Color = hex(0xe9e4d4);
const GOLD: Color = hex(0xd9ad52);
const LIGHT_GOLD: Color = hex(0xedd08a);
const SAGE: Color = hex(0x8ba7a3);
const RUBRIC: Color = hex(0xc4576a);

pub const VESPERS: Theme = Theme {
    name: "vespers",
    background: DEPTH,
    surface: SURFACE,
    border: RAISED,
    text_primary: PARCHMENT,
    text_muted: SLATE,
    accent: GOLD,
    accent_strong: LIGHT_GOLD,
    selection_bg: LINES,
    selection_fg: PARCHMENT,
    unread_marker: GOLD,
    count: SLATE,
    status_ok: SAGE,
    status_error: RUBRIC,
    diff_add: SAGE,
    diff_remove: RUBRIC,
};

impl Theme {
    pub fn by_name(name: &str) -> Option<&'static Theme> {
        Self::all().find(|theme| theme.name == name)
    }

    pub fn names() -> impl Iterator<Item = &'static str> {
        Self::all().map(|theme| theme.name)
    }

    fn all() -> impl Iterator<Item = &'static Theme> {
        std::iter::once(&VESPERS).chain(gallery::GALLERY.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(colour: Color) -> u32 {
        let Color::Rgb(r, g, b) = colour else {
            panic!("expected an rgb colour, got {colour:?}");
        };
        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    }

    #[test]
    fn hex_round_trips() {
        let samples =
            [0x000000, 0xffffff, 0x0c0f1d, 0x123456, 0xd9ad52];
        for value in samples {
            assert_eq!(rgb(hex(value)), value);
        }
    }

    #[test]
    fn every_named_theme_resolves() {
        let names: Vec<_> = Theme::names().collect();
        assert_eq!(
            names,
            [
                "vespers",
                "kanagawa-wave",
                "catppuccin-mocha",
                "gruvbox-dark",
                "tokyo-night",
                "nord",
                "rose-pine",
            ],
        );
        for name in names {
            let theme = Theme::by_name(name)
                .unwrap_or_else(|| panic!("{name} did not resolve"));
            assert_eq!(theme.name, name);
        }
    }

    #[test]
    fn unknown_names_return_none() {
        assert!(Theme::by_name("").is_none());
        assert!(Theme::by_name("solarized").is_none());
        assert!(Theme::by_name("Vespers").is_none());
    }

    #[test]
    fn vespers_matches_the_branding_palette() {
        let vespers = Theme::by_name("vespers").unwrap();
        let slots = [
            (vespers.background, 0x0c0f1d),
            (vespers.surface, 0x141a2e),
            (vespers.border, 0x1e2742),
            (vespers.text_primary, 0xe9e4d4),
            (vespers.text_muted, 0x7a86ad),
            (vespers.accent, 0xd9ad52),
            (vespers.accent_strong, 0xedd08a),
            (vespers.selection_bg, 0x2c3860),
            (vespers.selection_fg, 0xe9e4d4),
            (vespers.unread_marker, 0xd9ad52),
            (vespers.count, 0x7a86ad),
            (vespers.status_ok, 0x8ba7a3),
            (vespers.status_error, 0xc4576a),
            (vespers.diff_add, 0x8ba7a3),
            (vespers.diff_remove, 0xc4576a),
        ];
        for (slot, expected) in slots {
            assert_eq!(rgb(slot), expected);
        }
    }
}
