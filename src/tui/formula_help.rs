//! Help while a formula is typed: the function the cursor is in (with the
//! current argument marked), the functions a started name may become, or a
//! short reference of what formulas can contain.

use crate::builtins::{ALIASES, BUILTINS};

/// A function to explain: built-in, or one of the document's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Func {
    pub name: String,
    /// `sumif(range; criterion; [sum_range])`
    pub signature: String,
    pub desc: String,
}

/// One line of the help: a label, then text pieces (`true`: marked).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub label: String,
    pub parts: Vec<(String, bool)>,
}

fn line(label: &str, text: &str) -> Line {
    Line { label: label.to_string(), parts: vec![(text.to_string(), false)] }
}

/// What formulas can contain, shown while no function is concerned.
const BASICS: &[(&str, &str)] = &[
    ("References", "B2 · B2:B4 · B (whole column) · Amount (column by name) · Expenses!B2 · $B$1 stays when filled"),
    ("Pointing", "after = ( ; or an operator: arrows or a click insert a reference, Shift or dragging a range"),
    ("Operators", "+ − * / ^ · 19% · & joins texts · = <> < <= > >= compare"),
    ("Functions", "sum(B) · if(B2 > 100; \"high\"; \"low\") · arguments are separated by ; · type a name for the list"),
];

/// The built-in functions and the document's own `user` ones.
pub fn all_functions(user: &[Func]) -> Vec<Func> {
    let builtins = BUILTINS.iter().map(|b| Func { name: b.name.into(), signature: b.signature.into(), desc: b.desc.into() });
    user.iter().cloned().chain(builtins).collect()
}

/// The function call the cursor is in: its name and the argument index.
fn enclosing_call(before: &str) -> Option<(String, usize)> {
    // Open brackets: the name before `(` (`None` for grouping and `[`), the `;` seen.
    let mut stack: Vec<(Option<String>, usize)> = Vec::new();
    let mut word = String::new();
    let mut in_text = false;
    for c in before.chars() {
        if in_text {
            in_text = c != '"';
            continue;
        }
        match c {
            '"' => in_text = true,
            c if c.is_alphanumeric() || c == '_' || c == '.' => {
                word.push(c);
                continue;
            }
            '(' => stack.push(((!word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit())).then(|| word.clone()), 0)),
            '[' => stack.push((None, 0)),
            ')' | ']' => {
                stack.pop();
            }
            ';' => {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
            }
            _ => {}
        }
        word.clear();
    }
    stack.into_iter().rev().find_map(|(name, arg)| name.map(|n| (n, arg)))
}

/// The name being typed right before the cursor (letters, digits, `_`), if
/// it starts with a letter.
fn started_name(before: &str) -> Option<String> {
    let tail: String = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
    tail.starts_with(char::is_alphabetic).then_some(tail)
}

/// The signature with argument `arg` marked; the last one repeats when it
/// is `…` (or ends with it).
fn marked_signature(signature: &str, arg: usize) -> Vec<(String, bool)> {
    let (Some(open), Some(close)) = (signature.find('('), signature.rfind(')')) else { return vec![(signature.to_string(), false)] };
    let params: Vec<&str> = signature[open + 1..close].split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    if params.is_empty() {
        return vec![(signature.to_string(), false)];
    }
    let repeats = params.last().is_some_and(|p| p.ends_with('…'));
    let at = if arg < params.len() {
        Some(arg)
    } else if repeats {
        Some(params.len() - 1)
    } else {
        None
    };
    let mut out = vec![(signature[..=open].to_string(), false)];
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push(("; ".into(), false));
        }
        out.push((p.to_string(), Some(i) == at));
    }
    out.push((signature[close..].to_string(), false));
    out
}

/// The function named `name` (case does not matter; aliases resolved).
fn find<'a>(funcs: &'a [Func], name: &str) -> Option<&'a Func> {
    let lower = name.to_lowercase();
    let name = ALIASES.iter().find(|(a, _)| *a == lower).map_or(lower.as_str(), |(_, n)| n);
    funcs.iter().find(|f| f.name.to_lowercase() == name)
}

/// The help for `buf` with the cursor at character `cursor`, at most `max`
/// lines. `basics`: without a function concerned, the short reference.
pub fn help(buf: &str, cursor: usize, funcs: &[Func], max: usize, basics: bool) -> Vec<Line> {
    let before: String = buf.chars().take(cursor).collect();
    let mut out = Vec::new();
    if let Some((name, arg)) = enclosing_call(&before)
        && let Some(f) = find(funcs, &name)
    {
        let mut parts = marked_signature(&f.signature, arg);
        parts.push((format!("  {}", f.desc), false));
        out.push(Line { label: "In".into(), parts });
    }
    // Inside a text nothing is a name.
    let in_text = before.matches('"').count() % 2 == 1;
    if let Some(typed) = started_name(&before).filter(|_| !in_text) {
        let lower = typed.to_lowercase();
        let mut matches: Vec<&Func> = funcs.iter().filter(|f| f.name.to_lowercase().starts_with(&lower)).collect();
        // Other names (German, Excel) lead to their function.
        for (alias, target) in ALIASES {
            if alias.starts_with(&lower)
                && let Some(f) = find(funcs, target)
                && !matches.iter().any(|m| m.name == f.name)
            {
                matches.push(f);
            }
        }
        // Only the name typed in full: it is the call shown above, or a column.
        let exact = matches.len() == 1 && matches[0].name.eq_ignore_ascii_case(&typed);
        if !matches.is_empty() && !exact {
            let room = max.saturating_sub(out.len());
            let more = matches.len().saturating_sub(room);
            let shown = if more > 0 { room.saturating_sub(1) } else { room };
            for (k, f) in matches.iter().take(shown).enumerate() {
                let label = if k == 0 { "Functions" } else { "" };
                out.push(Line { label: label.into(), parts: vec![(f.signature.clone(), true), (format!("  {}", f.desc), false)] });
            }
            if more > 0 && shown < matches.len() {
                let rest: Vec<&str> = matches[shown..].iter().map(|f| f.name.as_str()).collect();
                out.push(line(if shown == 0 { "Functions" } else { "" }, &format!("also {}", rest.join(", "))));
            }
        }
    }
    if out.is_empty() && basics {
        out.extend(BASICS.iter().take(max).map(|(l, t)| line(l, t)));
    }
    out.truncate(max);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.parts.iter().map(|(t, _)| t.as_str()).collect()).collect()
    }

    fn marked(l: &Line) -> Vec<&str> {
        l.parts.iter().filter(|(_, m)| *m).map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn the_argument_at_the_cursor_is_marked() {
        let funcs = all_functions(&[]);
        let h = help("=sumif(A; \">5\"; ", 16, &funcs, 6, true);
        assert_eq!(h[0].label, "In");
        assert_eq!(marked(&h[0]), ["[sum_range]"]);
        // A `;` inside a text and a nested call do not count for the outer one.
        let h = help("=if(len(\"a;b\") > 2; ", 20, &funcs, 6, true);
        assert_eq!(marked(&h[0]), ["then"]);
        let h = help("=if(len(\"a;su", 13, &funcs, 6, true);
        assert_eq!(text(&h).len(), 1, "no names in a text");
        assert!(text(&h)[0].starts_with("len(text)"));
        // Repeating arguments: the last one stays marked.
        let h = help("=sum(A; B; C; ", 14, &funcs, 6, true);
        assert_eq!(marked(&h[0]), ["values …"]);
        // Aliases and upper case find the function.
        let h = help("=SUMME(", 7, &funcs, 6, true);
        assert!(text(&h)[0].starts_with("sum("));
    }

    #[test]
    fn a_started_name_lists_the_functions() {
        let own = [Func { name: "gross".into(), signature: "gross(x)".into(), desc: "with VAT".into() }];
        let funcs = all_functions(&own);
        let h = help("=B2 * gr", 8, &funcs, 6, true);
        assert_eq!(text(&h), ["gross(x)  with VAT"]);
        let h = help("=count", 6, &funcs, 6, true);
        let names: Vec<String> = text(&h);
        assert!(names[0].starts_with("count(") && names.iter().any(|n| n.starts_with("countifs(")), "{names:?}");
        // More than there is room for: the rest by name.
        let h = help("=s", 2, &funcs, 3, true);
        assert_eq!(h.len(), 3);
        assert!(text(&h)[2].starts_with("also "), "{:?}", text(&h));
        // A column name matching nothing: the basics.
        let h = help("=Amount", 7, &funcs, 6, true);
        assert_eq!(h[0].label, "References");
        assert!(help("=Amount", 7, &funcs, 6, false).is_empty());
    }

    #[test]
    fn signatures_are_split_into_arguments() {
        let m = marked_signature("pi()", 0);
        assert_eq!(m, [("pi()".to_string(), false)]);
        let m = marked_signature("mid(text; start; n)", 5);
        assert!(m.iter().all(|(_, marked)| !marked), "past the last argument: none");
    }
}
