const DIFF_GIT: &str = "diff --git ";
const OLD_FILE: &str = "--- ";
const NEW_FILE: &str = "+++ ";
const HUNK_OPEN: &str = "@@ -";
const HUNK_CLOSE: &str = " @@";
const NO_NEWLINE: &str = "\\";
const ENVELOPE_SEPARATOR: &str = "---";
const ENVELOPE_SIGNATURE: &str = "--";
const DEFAULT_SPAN: u64 = 1;

const FILE_HEADER_PREFIXES: [&str; 14] = [
    DIFF_GIT,
    OLD_FILE,
    NEW_FILE,
    "index ",
    "old mode ",
    "new mode ",
    "new file mode ",
    "deleted file mode ",
    "similarity index ",
    "dissimilarity index ",
    "rename from ",
    "rename to ",
    "copy from ",
    "copy to ",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchLine {
    Text,
    FileHeader,
    HunkHeader,
    Addition,
    Removal,
    NoNewline,
    Envelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Prose,
    Diff,
    Hunk { old_left: u64, new_left: u64 },
}

pub fn classify_patch(body: &str) -> Vec<PatchLine> {
    let lines: Vec<&str> = body.lines().collect();
    let mut kinds = Vec::with_capacity(lines.len());
    let mut state = State::Prose;
    for (index, line) in lines.iter().enumerate() {
        let next = lines.get(index + 1).copied();
        let (kind, moved) = step(state, line, next);
        kinds.push(kind);
        state = moved;
    }
    kinds
}

pub fn is_patch(subject: &str, body: &str) -> bool {
    has_patch_tag(subject) || body_has_diff(body)
}

pub(crate) fn has_patch_tag(subject: &str) -> bool {
    bracket_tags(subject).any(|tag| {
        tag.split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("patch"))
    })
}

pub(crate) fn body_has_diff(body: &str) -> bool {
    if body.lines().any(|line| line.starts_with(DIFF_GIT)) {
        return true;
    }
    classify_patch(body).contains(&PatchLine::HunkHeader)
}

pub(crate) fn bracket_tags(
    subject: &str,
) -> impl Iterator<Item = &str> {
    subject
        .split('[')
        .skip(1)
        .filter_map(|rest| rest.split_once(']').map(|(tag, _)| tag))
}

fn step(
    state: State,
    line: &str,
    next: Option<&str>,
) -> (PatchLine, State) {
    match state {
        State::Hunk { old_left, new_left } => {
            hunk_step(old_left, new_left, line)
                .unwrap_or_else(|| diff_step(line, next))
        }
        State::Diff => diff_step(line, next),
        State::Prose => prose_step(line, next),
    }
}

fn hunk_step(
    old_left: u64,
    new_left: u64,
    line: &str,
) -> Option<(PatchLine, State)> {
    let (kind, old_used, new_used) = match line.as_bytes().first() {
        Some(b'+') => (PatchLine::Addition, 0, 1),
        Some(b'-') => (PatchLine::Removal, 1, 0),
        Some(b'\\') => (PatchLine::NoNewline, 0, 0),
        Some(b' ') | None => (PatchLine::Text, 1, 1),
        _ => return None,
    };
    let left = (
        old_left.saturating_sub(old_used),
        new_left.saturating_sub(new_used),
    );
    Some((kind, hunk_state(left)))
}

fn diff_step(line: &str, next: Option<&str>) -> (PatchLine, State) {
    if let Some(spans) = hunk_spans(line) {
        return (PatchLine::HunkHeader, hunk_state(spans));
    }
    if is_file_header(line) {
        return (PatchLine::FileHeader, State::Diff);
    }
    if line.starts_with(NO_NEWLINE) {
        return (PatchLine::NoNewline, State::Diff);
    }
    prose_step(line, next)
}

fn prose_step(line: &str, next: Option<&str>) -> (PatchLine, State) {
    if line.starts_with(DIFF_GIT) {
        return (PatchLine::FileHeader, State::Diff);
    }
    if opens_file_pair(line, next) {
        return (PatchLine::FileHeader, State::Diff);
    }
    if is_envelope(line) {
        return (PatchLine::Envelope, State::Prose);
    }
    (PatchLine::Text, State::Prose)
}

fn is_file_header(line: &str) -> bool {
    FILE_HEADER_PREFIXES
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn opens_file_pair(line: &str, next: Option<&str>) -> bool {
    line.starts_with(OLD_FILE)
        && next.is_some_and(|line| line.starts_with(NEW_FILE))
}

fn is_envelope(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed == ENVELOPE_SEPARATOR || trimmed == ENVELOPE_SIGNATURE
}

fn hunk_state((old_left, new_left): (u64, u64)) -> State {
    if old_left == 0 && new_left == 0 {
        return State::Diff;
    }
    State::Hunk { old_left, new_left }
}

fn hunk_spans(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix(HUNK_OPEN)?;
    let (old, rest) = line_span(rest)?;
    let rest = rest.strip_prefix(" +")?;
    let (new, rest) = line_span(rest)?;
    if !rest.starts_with(HUNK_CLOSE) {
        return None;
    }
    Some((old, new))
}

fn line_span(text: &str) -> Option<(u64, &str)> {
    let (_, rest) = leading_number(text)?;
    match rest.strip_prefix(',') {
        None => Some((DEFAULT_SPAN, rest)),
        Some(tail) => leading_number(tail),
    }
}

fn leading_number(text: &str) -> Option<(u64, &str)> {
    let end = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    let value = text[..end].parse().ok()?;
    Some((value, &text[end..]))
}

#[cfg(test)]
mod tests {
    use super::PatchLine::{
        Addition, Envelope, FileHeader, HunkHeader, NoNewline, Removal,
        Text,
    };
    use super::*;

    const FORMAT_PATCH_BODY: &str = concat!(
        "Teach the widget to sing.\n",
        "---\n",
        " widget.c | 3 ++-\n",
        " 1 file changed, 2 insertions(+), 1 deletion(-)\n",
        "\n",
        "diff --git a/widget.c b/widget.c\n",
        "index 1111111..2222222 100644\n",
        "--- a/widget.c\n",
        "+++ b/widget.c\n",
        "@@ -1,3 +1,4 @@ int main(void)\n",
        " keep\n",
        "-drop\n",
        "+sing\n",
        "+encore\n",
        " tail\n",
        "\\ No newline at end of file\n",
        "-- \n",
        "2.45.0\n",
    );

    #[test]
    fn classifies_a_format_patch_body_line_by_line() {
        let expected = [
            Text, Envelope, Text, Text, Text, FileHeader, FileHeader,
            FileHeader, FileHeader, HunkHeader, Text, Removal,
            Addition, Addition, Text, NoNewline, Envelope, Text,
        ];
        assert_eq!(classify_patch(FORMAT_PATCH_BODY), expected);
    }

    #[test]
    fn file_headers_are_never_additions_or_removals() {
        let kinds = classify_patch(FORMAT_PATCH_BODY);
        assert_eq!(kinds[7], FileHeader, "--- a/widget.c");
        assert_eq!(kinds[8], FileHeader, "+++ b/widget.c");
    }

    #[test]
    fn hunk_counts_bound_the_coloured_region() {
        let body = concat!(
            "--- a/one\n",
            "+++ b/one\n",
            "@@ -1,1 +1,1 @@\n",
            "-old\n",
            "+new\n",
            "-not part of the hunk\n",
            "+1 for this idea\n",
        );
        let expected = [
            FileHeader, FileHeader, HunkHeader, Removal, Addition,
            Text, Text,
        ];
        assert_eq!(classify_patch(body), expected);
    }

    #[test]
    fn prose_plus_and_minus_lines_stay_text() {
        let body = concat!(
            "+1 from me\n",
            "- a list item\n",
            "@@ not a hunk @@\n",
        );
        assert_eq!(classify_patch(body), [Text, Text, Text]);
    }

    #[test]
    fn zero_span_hunks_parse() {
        let body = concat!(
            "diff --git a/f b/f\n",
            "new file mode 100644\n",
            "--- /dev/null\n",
            "+++ b/f\n",
            "@@ -0,0 +1 @@\n",
            "+only\n",
        );
        let expected = [
            FileHeader, FileHeader, FileHeader, FileHeader, HunkHeader,
            Addition,
        ];
        assert_eq!(classify_patch(body), expected);
    }

    #[test]
    fn detection_covers_subjects_and_bodies() {
        let diff_body = concat!(
            "--- a/f\n",
            "+++ b/f\n",
            "@@ -1 +1 @@\n",
            "-a\n",
            "+b\n",
        );
        let cases: &[(&str, &str, bool)] = &[
            ("[PATCH] fix the widget", "prose", true),
            ("[PATCH v3 2/7] fix", "prose", true),
            ("[RFC PATCH] idea", "prose", true),
            ("[patch] lower case", "prose", true),
            ("Re: [PATCH 1/2] fix", "prose", true),
            ("plain subject", diff_body, true),
            ("plain subject", "diff --git a/f b/f\n", true),
            ("plain subject", "prose only\n", false),
            ("[PATCHWORK] digest", "prose", false),
            ("no brackets PATCH here", "prose", false),
            ("dinner [tonight]", "see you at 8\n", false),
            ("maths", "a --- b\n+++ result\n", false),
        ];
        for (subject, body, expected) in cases {
            assert_eq!(
                is_patch(subject, body),
                *expected,
                "subject `{subject}` body `{body}`"
            );
        }
    }

    #[test]
    fn bare_separators_read_as_envelope() {
        let body = "---\ndiffstat here\n-- \nsig\n";
        let kinds = classify_patch(body);
        assert_eq!(kinds, [Envelope, Text, Envelope, Text]);
    }
}
