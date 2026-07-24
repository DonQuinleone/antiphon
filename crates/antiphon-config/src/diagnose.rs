use std::path::Path;

use crate::error::ConfigError;

const SUGGESTION_FLOOR: f64 = 0.7;

pub(crate) fn enrich(
    err: toml::de::Error,
    text: &str,
    path: &Path,
) -> ConfigError {
    let message = err.message().to_string();
    ConfigError {
        file: path.to_path_buf(),
        line: err.span().map(|span| line_of(text, span.start)),
        suggestion: suggest(&message),
        message,
    }
}

fn line_of(text: &str, offset: usize) -> usize {
    let upto = offset.min(text.len());
    text[..upto].bytes().filter(|byte| *byte == b'\n').count() + 1
}

pub(crate) fn locate_key(text: &str, key: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            let line = line.trim_start();
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
                || line.starts_with(&format!("[{key}"))
        })
        .map(|index| index + 1)
}

pub(crate) fn suggest(message: &str) -> Option<String> {
    let unknown = unknown_key(message)?;
    let candidates = expected_keys(message);
    candidates
        .into_iter()
        .map(|key| (strsim::jaro_winkler(&unknown, &key), key))
        .filter(|(score, _)| *score >= SUGGESTION_FLOOR)
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, key)| key)
}

pub(crate) fn unknown_key(message: &str) -> Option<String> {
    let rest = message.strip_prefix("unknown field `")?;
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn expected_keys(message: &str) -> Vec<String> {
    let Some(at) = message.find("expected") else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    let mut rest = &message[at..];
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else {
            break;
        };
        keys.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    keys
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::account::AccountFile;
    use crate::error::ConfigError;
    use crate::load::parse;
    use crate::schema::Config;

    fn config_error(text: &str) -> ConfigError {
        parse::<Config>(text, Path::new("config.toml"))
            .expect_err("expected a config error")
    }

    struct Rejection {
        name: &'static str,
        text: &'static str,
        line: usize,
        suggestion: Option<&'static str>,
    }

    const REJECTIONS: &[Rejection] = &[
        Rejection {
            name: "top-level typo",
            text: "[notification]\nenabled = true\n",
            line: 1,
            suggestion: Some("notifications"),
        },
        Rejection {
            name: "nested typo",
            text: "[ui]\nreadin_pane = \"below\"\n",
            line: 2,
            suggestion: Some("reading_pane"),
        },
        Rejection {
            name: "unrelated key gets no guess",
            text: "[ui]\nzzz_qqq = 1\n",
            line: 2,
            suggestion: None,
        },
    ];

    #[test]
    fn unknown_keys_fail_with_line_and_suggestion() {
        for case in REJECTIONS {
            let err = config_error(case.text);
            assert_eq!(
                err.line,
                Some(case.line),
                "line for {}",
                case.name
            );
            assert_eq!(
                err.suggestion.as_deref(),
                case.suggestion,
                "suggestion for {}",
                case.name
            );
        }
    }

    #[test]
    fn account_typos_get_suggestions() {
        let text = "[account]\nname = \"a\"\n[imap]\n\
                    host = \"h\"\nuser = \"u\"\n\
                    pasword_cmd = \"pass\"\n";
        let err = parse::<AccountFile>(text, Path::new("a.toml"))
            .expect_err("expected an account error");
        assert_eq!(err.line, Some(6));
        assert_eq!(err.suggestion.as_deref(), Some("password_cmd"));
    }

    #[test]
    fn wrong_types_report_their_line() {
        let err =
            config_error("[vault]\nidle_lock_minutes = \"never\"\n");
        assert_eq!(err.line, Some(2));
        assert!(err.message.contains("invalid type"));
    }
}
