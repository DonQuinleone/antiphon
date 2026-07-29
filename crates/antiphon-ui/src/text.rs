pub const ELLIPSIS: char = '\u{2026}';

/// Text clipped to a column: anything longer than `width` ends
/// in a visible ellipsis rather than being chopped into the
/// next column. A zero width yields nothing.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut kept: String = text.chars().take(width - 1).collect();
    kept.push(ELLIPSIS);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_is_exact_at_every_width() {
        let cases = [
            ("hello", 10, "hello".to_string()),
            ("hello", 5, "hello".to_string()),
            ("hello!", 5, format!("hell{ELLIPSIS}")),
            ("hi", 1, ELLIPSIS.to_string()),
            ("hi", 0, String::new()),
        ];
        for (text, width, expected) in cases {
            assert_eq!(
                truncate(text, width),
                expected,
                "{text}@{width}"
            );
        }
    }
}
