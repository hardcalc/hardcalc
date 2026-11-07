//! Splits a TOML file into lines and finds the `#%` lines (schema and
//! diagnostics), skipping lines that lie inside multi-line strings.

/// One line of the file, including its line break.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line<'a> {
    /// 1-based line number.
    pub number: usize,
    pub text: &'a str,
    /// The line starts inside a `"""…"""` or `'''…'''` string.
    pub in_string: bool,
}

impl<'a> Line<'a> {
    /// Content after `#%` for a schema or diagnostic line.
    pub fn schema(&self) -> Option<&'a str> {
        if self.in_string {
            return None;
        }
        self.text.strip_prefix("#%").map(|s| s.trim_end_matches(['\n', '\r']))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Normal,
    /// Inside `"""`.
    Basic,
    /// Inside `'''`.
    Literal,
}

pub fn lines(text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut state = State::Normal;
    for (i, text) in text.split_inclusive('\n').enumerate() {
        out.push(Line { number: i + 1, text, in_string: state != State::Normal });
        state = scan(text, state);
    }
    out
}

/// Advances the string state over one line.
fn scan(line: &str, mut state: State) -> State {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match state {
            State::Basic => {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i..].starts_with(b"\"\"\"") {
                    // Up to two more quotes belong to the content: `""""` ends with one quote in the value.
                    i += 3;
                    while i < b.len() && b[i] == b'"' {
                        i += 1;
                    }
                    state = State::Normal;
                    continue;
                }
                i += 1;
            }
            State::Literal => {
                if b[i..].starts_with(b"'''") {
                    i += 3;
                    while i < b.len() && b[i] == b'\'' {
                        i += 1;
                    }
                    state = State::Normal;
                    continue;
                }
                i += 1;
            }
            State::Normal => match b[i] {
                b'#' => break,
                b'"' if b[i..].starts_with(b"\"\"\"") => {
                    state = State::Basic;
                    i += 3;
                }
                b'\'' if b[i..].starts_with(b"'''") => {
                    state = State::Literal;
                    i += 3;
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                    i += 1;
                }
                _ => i += 1,
            },
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_multiline_strings() {
        let text = "a = \"\"\"\n#%error x\n\"\"\"\n#%hardconf 1\nb = '''x\n#% y'''\nc = \"#%\" # \"\"\"\n#%types\n";
        let schema: Vec<&str> = lines(text).iter().filter_map(|l| l.schema()).collect();
        assert_eq!(schema, ["hardconf 1", "types"]);
    }
}
