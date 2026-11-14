//! Markdown text for printing: live values, inline emphasis and word wrap.

use crate::engine::Workbook;
use crate::layers::CellStyle;
use unicode_width::UnicodeWidthStr;

/// Replaces `{=expr}` and `{=expr | format}` by the computed value. Plain
/// names refer to table `sheet` (the one above the text). `format` uses the
/// format-layer syntax, e.g. `percent(1)` or `decimals(2)`.
pub fn live_values(line: &str, wb: &mut Workbook, sheet: usize) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("{=") {
        out.push_str(&rest[..start]);
        let body = &rest[start + 2..];
        // Matching `}`: unit literals like `5{m}` may nest braces.
        let mut depth = 0;
        let end = body.char_indices().find_map(|(i, c)| match c {
            '{' => {
                depth += 1;
                None
            }
            '}' if depth == 0 => Some(i),
            '}' => {
                depth -= 1;
                None
            }
            _ => None,
        });
        let Some(end) = end else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &body[..end];
        let (expr, format) = match inner.rsplit_once(" | ") {
            Some((e, f)) => (e, Some(f)),
            None => (inner, None),
        };
        let value = wb.eval_formula(expr, sheet);
        let style = format.map(|f| CellStyle::parse(f).0).unwrap_or_default();
        out.push_str(&style.display(&value, wb.decimal_comma).unwrap_or_else(|| value.display(None, wb.decimal_comma)));
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A piece of text with emphasis.
#[derive(Debug, Clone, PartialEq)]
pub struct Styled {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

/// Splits `**bold**` and `*italic*`; other Markdown is kept as text, except
/// that `[text](url)` becomes `text`, inline code loses its backticks and
/// `$…$` math becomes Unicode (`$\\gamma_M$` → γ_M).
pub fn inline(line: &str) -> Vec<Styled> {
    let line = crate::symbols::inline_math(&strip_links(line).replace('`', ""));
    let mut out: Vec<Styled> = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let mut cur = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut flush = |cur: &mut String, bold: bool, italic: bool| {
        if !cur.is_empty() {
            out.push(Styled { text: std::mem::take(cur), bold, italic });
        }
    };
    while i < chars.len() {
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            flush(&mut cur, bold, italic);
            bold = !bold;
            i += 2;
        } else if chars[i] == '*' && (italic || chars.get(i + 1).is_some_and(|c| !c.is_whitespace())) {
            flush(&mut cur, bold, italic);
            italic = !italic;
            i += 1;
        } else {
            cur.push(chars[i]);
            i += 1;
        }
    }
    flush(&mut cur, bold, italic);
    out
}

fn strip_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find("](")
            && let Some(end) = after[close + 2..].find(')')
        {
            out.push_str(&rest[..open]);
            out.push_str(&after[..close]);
            rest = &after[close + 2 + end + 1..];
        } else {
            out.push_str(&rest[..=open]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Word-wraps styled text to `width` columns; `indent` columns are kept free
/// on continuation lines (for list items).
pub fn wrap(pieces: &[Styled], width: usize, indent: usize) -> Vec<Vec<Styled>> {
    let mut lines: Vec<Vec<Styled>> = vec![Vec::new()];
    let mut used = 0;
    for piece in pieces {
        for (k, word) in piece.text.split(' ').enumerate() {
            // Words after the first were separated by a space (a leading space
            // shows up as an empty first word).
            let space = usize::from(k > 0);
            let w = word.width();
            if used > indent && used + space + w > width {
                lines.push(vec![Styled { text: " ".repeat(indent), bold: false, italic: false }]);
                used = indent;
            } else if space > 0 && used > 0 {
                push(lines.last_mut().unwrap(), " ", piece);
                used += 1;
            }
            if !word.is_empty() {
                push(lines.last_mut().unwrap(), word, piece);
                used += w;
            }
        }
    }
    lines
}

fn push(line: &mut Vec<Styled>, text: &str, style: &Styled) {
    match line.last_mut() {
        Some(last) if last.bold == style.bold && last.italic == style.italic => last.text.push_str(text),
        _ => line.push(Styled { text: text.to_string(), bold: style.bold, italic: style.italic }),
    }
}

/// Superscript digits for footnote markers.
pub fn superscript(n: usize) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    n.to_string().chars().map(|c| DIGITS[c.to_digit(10).unwrap() as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn values_in_text() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let doc = parse(include_str!("../examples/budget.hcmd")).unwrap();
        let mut wb = Workbook::from_document(&doc).0;
        assert_eq!(
            live_values("Rent: {=Expenses[Rent, Share] | percent(1)} – {=sum(Expenses.Amount)}", &mut wb, 0),
            "Rent: 90,5 % – 1325,49 €"
        );
        assert_eq!(live_values("{=2 * 3{m}}", &mut wb, 0), "6 m");
        assert!(live_values("{=nope}", &mut wb, 0).starts_with("#ERR"));
        assert_eq!(live_values("no {= end", &mut wb, 0), "no {= end");
    }

    #[test]
    fn emphasis_and_links() {
        let p = inline("a **b** *c* [d](http://x) `e` 2 * 3");
        let texts: Vec<(&str, bool, bool)> = p.iter().map(|s| (s.text.as_str(), s.bold, s.italic)).collect();
        assert_eq!(texts, [("a ", false, false), ("b", true, false), (" ", false, false), ("c", false, true), (" d e 2 * 3", false, false)]);
    }

    #[test]
    fn wrapping() {
        let lines = wrap(&inline("one two three four five"), 9, 2);
        let plain: Vec<String> = lines.iter().map(|l| l.iter().map(|s| s.text.as_str()).collect()).collect();
        assert_eq!(plain, ["one two", "  three", "  four", "  five"]);
        assert_eq!(superscript(12), "¹²");
        let joined: Vec<String> =
            wrap(&inline("a **b** c"), 40, 0).iter().map(|l| l.iter().map(|s| s.text.as_str()).collect()).collect();
        assert_eq!(joined, ["a b c"]);
    }
}

/// Breaks `s` at spaces into lines of at most `w` columns; a word longer than
/// a line is cut. Always at least one line.
pub fn wrap_plain(s: &str, w: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let w = w.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in s.split(' ').filter(|x| !x.is_empty()) {
        let ww = word.width();
        if used > 0 && used + 1 + ww <= w {
            line.push(' ');
            line.push_str(word);
            used += 1 + ww;
            continue;
        }
        if used > 0 {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        for c in word.chars() {
            let cw = c.width().unwrap_or(0);
            if used + cw > w {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += cw;
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod wrap_plain_tests {
    use super::wrap_plain;

    #[test]
    fn breaks_at_spaces_and_cuts_long_words() {
        assert_eq!(wrap_plain("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(wrap_plain("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap_plain("", 4), vec![""]);
    }
}
