const SIGNATURE_SEPARATOR: &str = "-- ";

pub(crate) fn unflow(text: &str, delsp: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut open_depth: Option<usize> = None;

    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let (depth, marked) = strip_quote_marks(line);
        let body = marked.strip_prefix(' ').unwrap_or(marked);
        let signature = body == SIGNATURE_SEPARATOR;
        let soft = body.ends_with(' ') && !signature;
        let continues = !signature && open_depth == Some(depth);
        let content = joinable(body, soft, delsp);

        append(&mut lines, continues, depth, content);
        open_depth = soft.then_some(depth);
    }

    lines.join("\n")
}

fn strip_quote_marks(line: &str) -> (usize, &str) {
    let depth = line.bytes().take_while(|&b| b == b'>').count();
    (depth, &line[depth..])
}

fn joinable(body: &str, soft: bool, delsp: bool) -> &str {
    if !(soft && delsp) {
        return body;
    }
    body.strip_suffix(' ').unwrap_or(body)
}

fn append(
    lines: &mut Vec<String>,
    continues: bool,
    depth: usize,
    content: &str,
) {
    match lines.last_mut() {
        Some(last) if continues => last.push_str(content),
        _ => lines.push(quoted(depth, content)),
    }
}

fn quoted(depth: usize, content: &str) -> String {
    if depth == 0 {
        return content.to_owned();
    }
    let mut line = ">".repeat(depth);
    if !content.is_empty() {
        line.push(' ');
    }
    line.push_str(content);
    line
}

#[cfg(test)]
mod tests {
    use super::unflow;

    #[test]
    fn unflows_for_display() {
        let cases = [
            (
                "soft break joins",
                "one two \nthree\n",
                false,
                "one two three\n",
            ),
            (
                "crlf soft break joins",
                "one \r\ntwo\r\n",
                false,
                "one two\n",
            ),
            (
                "delsp deletes the joining space",
                "flo \nwed\n",
                true,
                "flowed\n",
            ),
            (
                "hard breaks preserved",
                "para one\n\npara two\n",
                false,
                "para one\n\npara two\n",
            ),
            (
                "stuffed from line unstuffed",
                " From here\n",
                false,
                "From here\n",
            ),
            (
                "stuffed literal quote stays unquoted",
                " > not a quote\n",
                false,
                "> not a quote\n",
            ),
            (
                "quoted flowed lines join at same depth",
                "> line one \n> line two\n",
                false,
                "> line one line two\n",
            ),
            (
                "stuffed quote content matches spaced form",
                ">>Exit, Stage Left\n",
                false,
                ">> Exit, Stage Left\n",
            ),
            (
                "inner bracket is content, not depth",
                "> > Exit, Stage Left\n",
                false,
                "> > Exit, Stage Left\n",
            ),
            (
                "quoted delsp join",
                ">> delsp joi \n>> ned\n",
                true,
                ">> delsp joined\n",
            ),
            (
                "signature separator stays hard",
                "body text \n-- \nAlice\n",
                false,
                "body text \n-- \nAlice\n",
            ),
            (
                "quote depth wins over a flowed line",
                concat!(
                    "> Thou villainous ill-breeding spongy ",
                    "dizzy-eyed \n",
                    "> reeky elf-skinned pigeon-egg! \n",
                    ">> Thou artless swag-bellied ",
                    "milk-livered \n",
                    ">> dismal-dreaming idle-headed scut!\n",
                    ">>> Thou errant folly-fallen spleeny ",
                    "reeling-ripe \n",
                    ">>> unmuzzled ratsbane!\n",
                ),
                false,
                concat!(
                    "> Thou villainous ill-breeding spongy ",
                    "dizzy-eyed reeky elf-skinned ",
                    "pigeon-egg! \n",
                    ">> Thou artless swag-bellied ",
                    "milk-livered dismal-dreaming ",
                    "idle-headed scut!\n",
                    ">>> Thou errant folly-fallen spleeny ",
                    "reeling-ripe unmuzzled ratsbane!\n",
                ),
            ),
        ];
        for (name, input, delsp, expected) in cases {
            assert_eq!(unflow(input, delsp), expected, "case `{name}`");
        }
    }
}
