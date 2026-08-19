/// Convert untrusted repository bytes into printable terminal text.
///
/// C0/C1 controls, DEL, terminal escape introducers, invalid UTF-8, and common
/// bidirectional formatting controls are rendered visibly rather than passed
/// to the terminal.
pub fn sanitize_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        for character in chunk.valid().chars() {
            push_safe_char(&mut output, character);
        }
        for byte in chunk.invalid() {
            use std::fmt::Write as _;
            let _ = write!(output, "\\x{byte:02X}");
        }
    }
    output
}

pub fn sanitize_str(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        push_safe_char(&mut output, character);
    }
    output
}

fn push_safe_char(output: &mut String, character: char) {
    match character {
        '\n' => output.push_str("\\n"),
        '\r' => output.push_str("\\r"),
        '\t' => output.push_str("\\t"),
        '\u{1b}' => output.push_str("\\e"),
        value if value <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&value) => {
            use std::fmt::Write as _;
            let _ = write!(output, "\\u{{{:04X}}}", u32::from(value));
        }
        '\u{061c}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}' => {
            use std::fmt::Write as _;
            let _ = write!(output, "\\u{{{:04X}}}", u32::from(character));
        }
        value => output.push(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_controls_and_bidi() {
        assert_eq!(
            sanitize_str("ok\x1b]52;c;bad\x07\u{202e}"),
            "ok\\e]52;c;bad\\u{0007}\\u{202E}"
        );
    }

    #[test]
    fn invalid_utf8_is_visible() {
        assert_eq!(sanitize_bytes(&[b'a', 0xff, b'b']), "a\\xFFb");
    }
}
