pub(crate) fn parse(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) =
                pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect()
}

pub(crate) fn get<'a>(
    pairs: &'a [(String, String)],
    key: &str,
) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'+' {
            out.push(b' ');
            index += 1;
            continue;
        }
        if byte != b'%' {
            out.push(byte);
            index += 1;
            continue;
        }
        let Some(escaped) = decode_escape(bytes, index) else {
            out.push(b'%');
            index += 1;
            continue;
        };
        out.push(escaped);
        index += 3;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_escape(bytes: &[u8], index: usize) -> Option<u8> {
    let high = hex_value(*bytes.get(index + 1)?)?;
    let low = hex_value(*bytes.get(index + 2)?)?;
    Some((high << 4) | low)
}

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|value| value as u8)
}

#[cfg(test)]
mod tests {
    use super::{get, parse};

    #[test]
    fn parses_and_decodes_pairs() {
        let pairs = parse("code=4%2F0Ab-c&state=xy+z&empty=&flag");
        assert_eq!(get(&pairs, "code"), Some("4/0Ab-c"));
        assert_eq!(get(&pairs, "state"), Some("xy z"));
        assert_eq!(get(&pairs, "empty"), Some(""));
        assert_eq!(get(&pairs, "flag"), Some(""));
        assert_eq!(get(&pairs, "missing"), None);
    }

    #[test]
    fn keeps_malformed_escapes_literal() {
        let pairs = parse("code=%zz%4");
        assert_eq!(get(&pairs, "code"), Some("%zz%4"));
    }
}
