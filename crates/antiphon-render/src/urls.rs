pub(crate) const SCHEMES: [&str; 3] =
    ["https://", "http://", "mailto:"];

const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '\''];
const BRACKETS: [(char, char); 3] =
    [('(', ')'), ('[', ']'), ('{', '}')];

pub(crate) fn linkable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    SCHEMES.iter().any(|scheme| {
        lower.starts_with(scheme) && url.len() > scheme.len()
    })
}

pub(crate) fn scan_urls(line: &str) -> Vec<(usize, usize)> {
    let lower = line.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some((start, scheme)) = next_start(&lower, from) {
        let end = url_end(line, start);
        from = end.max(start + 1);
        if end > start + scheme.len() {
            found.push((start, end));
        }
    }
    found
}

fn next_start(
    lower: &str,
    mut from: usize,
) -> Option<(usize, &'static str)> {
    while from < lower.len() {
        let (start, scheme) = earliest_scheme(lower, from)?;
        if at_boundary(lower, start) {
            return Some((start, scheme));
        }
        from = start + 1;
    }
    None
}

fn earliest_scheme(
    lower: &str,
    from: usize,
) -> Option<(usize, &'static str)> {
    SCHEMES
        .iter()
        .filter_map(|scheme| {
            let hit = lower[from..].find(scheme)?;
            Some((from + hit, *scheme))
        })
        .min_by_key(|(start, _)| *start)
}

fn at_boundary(lower: &str, start: usize) -> bool {
    let before = lower[..start].chars().next_back();
    before.is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

fn url_end(line: &str, start: usize) -> usize {
    let rest = &line[start..];
    let stop = rest
        .char_indices()
        .find(|(_, ch)| is_stop(*ch))
        .map_or(line.len(), |(pos, _)| start + pos);
    trim_trailing(line, start, stop)
}

fn is_stop(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '<' | '>' | '"')
}

fn trim_trailing(line: &str, start: usize, mut end: usize) -> usize {
    loop {
        let candidate = &line[start..end];
        let Some(last) = candidate.chars().next_back() else {
            return end;
        };
        if !droppable(candidate, last) {
            return end;
        }
        end -= last.len_utf8();
    }
}

fn droppable(candidate: &str, last: char) -> bool {
    if TRAILING.contains(&last) {
        return true;
    }
    BRACKETS.iter().any(|&(open, close)| {
        last == close && unbalanced(candidate, open, close)
    })
}

fn unbalanced(text: &str, open: char, close: char) -> bool {
    text.matches(close).count() > text.matches(open).count()
}
