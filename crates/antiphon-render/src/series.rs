use crate::extract::body_text;
use crate::patch::{body_has_diff, bracket_tags, has_patch_tag};

const REPLY_PREFIX: &str = "re:";
const MBOX_FROM: &[u8] = b"From ";
const MBOX_SEPARATOR: &[u8] =
    b"From mboxrd@z Thu Jan  1 00:00:00 1970\n";
const DEFAULT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesMessage {
    pub subject: String,
    pub date_unix: i64,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PatchTag {
    version: u32,
    number: Option<u32>,
}

struct Candidate<'a> {
    message: &'a SeriesMessage,
    version: u32,
    number: Option<u32>,
}

pub fn patch_series(thread: &[SeriesMessage]) -> Vec<&SeriesMessage> {
    let mut candidates: Vec<Candidate> = thread
        .iter()
        .filter(|message| in_series(message))
        .map(|message| {
            let tag = patch_tag(&message.subject);
            Candidate {
                message,
                version: tag.version,
                number: tag.number,
            }
        })
        .collect();
    candidates.sort_by_key(|found| found.message.date_unix);
    let deduped = keep_latest_rolls(candidates);
    ordered(deduped)
}

pub fn mbox(series: &[&SeriesMessage]) -> Vec<u8> {
    let mut out = Vec::new();
    for message in series {
        out.extend_from_slice(MBOX_SEPARATOR);
        append_quoted(&mut out, &message.raw);
        out.push(b'\n');
    }
    out
}

fn in_series(message: &SeriesMessage) -> bool {
    if is_reply(&message.subject) {
        return false;
    }
    if has_patch_tag(&message.subject) {
        return true;
    }
    body_has_diff(&body_text(&message.raw).text)
}

fn is_reply(subject: &str) -> bool {
    subject
        .trim_start()
        .get(..REPLY_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(REPLY_PREFIX))
}

fn patch_tag(subject: &str) -> PatchTag {
    let mut parsed = PatchTag {
        version: DEFAULT_VERSION,
        number: None,
    };
    let Some(tag) = bracket_tags(subject).find(|tag| {
        tag.split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("patch"))
    }) else {
        return parsed;
    };
    for token in tag.split_whitespace() {
        if let Some(version) = version_token(token) {
            parsed.version = version;
            continue;
        }
        if let Some(number) = ordinal_token(token) {
            parsed.number = Some(number);
        }
    }
    parsed
}

fn version_token(token: &str) -> Option<u32> {
    let digits = token.strip_prefix(['v', 'V'])?;
    digits.parse().ok()
}

fn ordinal_token(token: &str) -> Option<u32> {
    let (number, total) = token.split_once('/')?;
    total.parse::<u32>().ok()?;
    number.parse().ok()
}

/// Re-rolls posted into the same thread: for one series
/// position, only the highest version survives; equal
/// versions resolve to the later posting.
fn keep_latest_rolls(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut kept: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        let slot = candidate.number.and_then(|number| {
            kept.iter().position(|held| held.number == Some(number))
        });
        let Some(slot) = slot else {
            kept.push(candidate);
            continue;
        };
        if candidate.version >= kept[slot].version {
            kept[slot] = candidate;
        }
    }
    kept
}

fn ordered<'a>(kept: Vec<Candidate<'a>>) -> Vec<&'a SeriesMessage> {
    let mut indexed: Vec<(usize, Candidate)> =
        kept.into_iter().enumerate().collect();
    indexed.sort_by_key(|(index, held)| match held.number {
        Some(number) => (0_u8, u64::from(number)),
        None => (1_u8, *index as u64),
    });
    indexed.into_iter().map(|(_, held)| held.message).collect()
}

fn append_quoted(out: &mut Vec<u8>, raw: &[u8]) {
    for line in raw.split_inclusive(|byte| *byte == b'\n') {
        if needs_quote(line) {
            out.push(b'>');
        }
        out.extend_from_slice(line);
    }
    if !raw.ends_with(b"\n") {
        out.push(b'\n');
    }
}

fn needs_quote(line: &[u8]) -> bool {
    let mut rest = line;
    while rest.first() == Some(&b'>') {
        rest = &rest[1..];
    }
    rest.starts_with(MBOX_FROM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        subject: &str,
        date_unix: i64,
        body: &str,
    ) -> SeriesMessage {
        let raw = format!(
            "From: dev@example.com\r\n\
             Subject: {subject}\r\n\
             Content-Type: text/plain\r\n\r\n{body}"
        );
        SeriesMessage {
            subject: subject.to_string(),
            date_unix,
            raw: raw.into_bytes(),
        }
    }

    const DIFF: &str = "---\ndiff --git a/f b/f\n@@ -1 +1 @@\n-a\n+b\n";

    #[test]
    fn series_selection_and_ordering() {
        let threads: &[(&str, Vec<SeriesMessage>, Vec<&str>)] = &[
            (
                "cover letter first, replies excluded",
                vec![
                    message("[PATCH 2/2] second", 20, DIFF),
                    message("Re: [PATCH 1/2] first", 30, "ack"),
                    message("[PATCH 0/2] cover", 5, "story"),
                    message("[PATCH 1/2] first", 10, DIFF),
                    message("plain chatter", 40, "hello"),
                ],
                vec![
                    "[PATCH 0/2] cover",
                    "[PATCH 1/2] first",
                    "[PATCH 2/2] second",
                ],
            ),
            (
                "a v2 re-roll shadows its v1",
                vec![
                    message("[PATCH 1/2] first", 10, DIFF),
                    message("[PATCH 2/2] second", 11, DIFF),
                    message("[PATCH v2 1/2] first again", 50, DIFF),
                ],
                vec![
                    "[PATCH v2 1/2] first again",
                    "[PATCH 2/2] second",
                ],
            ),
            (
                "unnumbered patches fall back to date order",
                vec![
                    message("[PATCH] later fix", 90, DIFF),
                    message("[PATCH] earlier fix", 15, DIFF),
                ],
                vec!["[PATCH] earlier fix", "[PATCH] later fix"],
            ),
            (
                "an untagged diff body still counts",
                vec![
                    message("a bare diff", 10, DIFF),
                    message("Re: a bare diff", 20, "thanks"),
                ],
                vec!["a bare diff"],
            ),
            (
                "no patches means an empty series",
                vec![message("dinner [tonight]", 10, "at 8")],
                vec![],
            ),
        ];
        for (name, thread, expected) in threads {
            let subjects: Vec<&str> = patch_series(thread)
                .iter()
                .map(|found| found.subject.as_str())
                .collect();
            assert_eq!(&subjects, expected, "{name}");
        }
    }

    #[test]
    fn mbox_separates_and_quotes() {
        let first = SeriesMessage {
            subject: "[PATCH 1/1] x".to_string(),
            date_unix: 1,
            raw: b"Subject: x\n\nFrom here on\n>From before\n".to_vec(),
        };
        let second = SeriesMessage {
            subject: "[PATCH] y".to_string(),
            date_unix: 2,
            raw: b"Subject: y\n\nno trailing newline".to_vec(),
        };
        let out = mbox(&[&first, &second]);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "From mboxrd@z Thu Jan  1 00:00:00 1970\n\
             Subject: x\n\n\
             >From here on\n\
             >>From before\n\n\
             From mboxrd@z Thu Jan  1 00:00:00 1970\n\
             Subject: y\n\n\
             no trailing newline\n\n"
        );
    }

    #[test]
    fn header_from_lines_are_not_quoted() {
        let raw = b"From: a@example.com\nSubject: s\n\nbody\n";
        let mut out = Vec::new();
        append_quoted(&mut out, raw);
        assert_eq!(out.as_slice(), raw.as_slice());
    }
}
