//! Special characters to insert while typing (Ctrl+G, `:symbol`): each with
//! its English name, its LaTeX name and the ASCII spellings it stands for
//! (`<=` for ≤). The LaTeX names are also what `\alpha` in a calculation
//! means.

/// A character that can be picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Symbol {
    pub ch: &'static str,
    pub name: &'static str,
    /// LaTeX names without the backslash (`le`, `leq`).
    pub latex: &'static [&'static str],
    /// ASCII spellings (`<=`).
    pub ascii: &'static [&'static str],
}

const fn s(ch: &'static str, name: &'static str, latex: &'static [&'static str], ascii: &'static [&'static str]) -> Symbol {
    Symbol { ch, name, latex, ascii }
}

/// All characters, in the order the list shows them.
pub const SYMBOLS: &[Symbol] = &[
    // Greek, small
    s("α", "alpha", &["alpha"], &[]),
    s("β", "beta", &["beta"], &[]),
    s("γ", "gamma", &["gamma"], &[]),
    s("δ", "delta", &["delta"], &[]),
    s("ε", "epsilon", &["varepsilon"], &[]),
    s("ϵ", "epsilon (lunate)", &["epsilon"], &[]),
    s("ζ", "zeta", &["zeta"], &[]),
    s("η", "eta", &["eta"], &[]),
    s("θ", "theta", &["theta"], &[]),
    s("ϑ", "theta (script)", &["vartheta"], &[]),
    s("ι", "iota", &["iota"], &[]),
    s("κ", "kappa", &["kappa"], &[]),
    s("λ", "lambda", &["lambda"], &[]),
    s("μ", "mu", &["mu"], &[]),
    s("ν", "nu", &["nu"], &[]),
    s("ξ", "xi", &["xi"], &[]),
    s("ο", "omicron", &["omicron"], &[]),
    s("π", "pi", &["pi"], &[]),
    s("ϖ", "pi (variant)", &["varpi"], &[]),
    s("ρ", "rho", &["rho"], &[]),
    s("ϱ", "rho (variant)", &["varrho"], &[]),
    s("σ", "sigma", &["sigma"], &[]),
    s("ς", "sigma (final)", &["varsigma"], &[]),
    s("τ", "tau", &["tau"], &[]),
    s("υ", "upsilon", &["upsilon"], &[]),
    s("φ", "phi", &["varphi"], &[]),
    s("ϕ", "phi (stroked)", &["phi"], &[]),
    s("χ", "chi", &["chi"], &[]),
    s("ψ", "psi", &["psi"], &[]),
    s("ω", "omega", &["omega"], &[]),
    // Greek, capital
    s("Α", "Alpha", &["Alpha"], &[]),
    s("Β", "Beta", &["Beta"], &[]),
    s("Γ", "Gamma", &["Gamma"], &[]),
    s("Δ", "Delta", &["Delta"], &[]),
    s("Ε", "Epsilon", &["Epsilon"], &[]),
    s("Ζ", "Zeta", &["Zeta"], &[]),
    s("Η", "Eta", &["Eta"], &[]),
    s("Θ", "Theta", &["Theta"], &[]),
    s("Ι", "Iota", &["Iota"], &[]),
    s("Κ", "Kappa", &["Kappa"], &[]),
    s("Λ", "Lambda", &["Lambda"], &[]),
    s("Μ", "Mu", &["Mu"], &[]),
    s("Ν", "Nu", &["Nu"], &[]),
    s("Ξ", "Xi", &["Xi"], &[]),
    s("Ο", "Omicron", &["Omicron"], &[]),
    s("Π", "Pi", &["Pi"], &[]),
    s("Ρ", "Rho", &["Rho"], &[]),
    s("Σ", "Sigma", &["Sigma"], &[]),
    s("Τ", "Tau", &["Tau"], &[]),
    s("Υ", "Upsilon", &["Upsilon"], &[]),
    s("Φ", "Phi", &["Phi"], &[]),
    s("Χ", "Chi", &["Chi"], &[]),
    s("Ψ", "Psi", &["Psi"], &[]),
    s("Ω", "Omega", &["Omega"], &[]),
    // Relations and operators
    s("≤", "less or equal", &["le", "leq"], &["<="]),
    s("≥", "greater or equal", &["ge", "geq"], &[">="]),
    s("≠", "not equal", &["ne", "neq"], &["<>", "!=", "/="]),
    s("≈", "approximately equal", &["approx"], &["~=", "~~"]),
    s("≡", "identical", &["equiv"], &["==="]),
    s("∼", "similar", &["sim"], &["~"]),
    s("∝", "proportional to", &["propto"], &[]),
    s("≪", "much less", &["ll"], &["<<"]),
    s("≫", "much greater", &["gg"], &[">>"]),
    s("±", "plus or minus", &["pm"], &["+-"]),
    s("∓", "minus or plus", &["mp"], &["-+"]),
    s("·", "dot (multiplication)", &["cdot"], &["*"]),
    s("×", "times", &["times"], &["x"]),
    s("÷", "divided by", &["div"], &[":"]),
    s("−", "minus", &[], &["-"]),
    s("√", "square root", &["sqrt", "surd"], &["sqrt"]),
    s("∛", "cube root", &[], &["cbrt"]),
    s("∞", "infinity", &["infty"], &["oo", "inf"]),
    s("∑", "sum", &["sum"], &[]),
    s("∏", "product", &["prod"], &[]),
    s("∫", "integral", &["int"], &[]),
    s("∮", "contour integral", &["oint"], &[]),
    s("∂", "partial", &["partial"], &[]),
    s("∇", "nabla", &["nabla"], &[]),
    s("∆", "increment", &[], &[]),
    s("∈", "element of", &["in"], &[]),
    s("∉", "not an element of", &["notin"], &[]),
    s("⊂", "subset", &["subset"], &[]),
    s("⊆", "subset or equal", &["subseteq"], &[]),
    s("∪", "union", &["cup"], &[]),
    s("∩", "intersection", &["cap"], &[]),
    s("∅", "empty set", &["emptyset", "varnothing"], &[]),
    s("∀", "for all", &["forall"], &[]),
    s("∃", "there exists", &["exists"], &[]),
    s("¬", "not", &["neg", "lnot"], &["!"]),
    s("∧", "and", &["wedge", "land"], &["&&", "/\\"]),
    s("∨", "or", &["vee", "lor"], &["||", "\\/"]),
    s("⊥", "perpendicular", &["perp"], &[]),
    s("∥", "parallel", &["parallel"], &[]),
    s("∠", "angle", &["angle"], &[]),
    s("°", "degree", &["circ", "degree"], &["deg"]),
    s("′", "prime (minutes, feet)", &["prime"], &["'"]),
    s("″", "double prime (seconds, inches)", &["dprime"], &["''"]),
    s("⌀", "diameter", &["diameter"], &["dia"]),
    // Arrows
    s("→", "right arrow", &["to", "rightarrow"], &["->"]),
    s("←", "left arrow", &["leftarrow", "gets"], &["<-"]),
    s("↔", "left right arrow", &["leftrightarrow"], &["<->"]),
    s("⇒", "implies", &["Rightarrow", "implies"], &["=>"]),
    s("⇐", "implied by", &["Leftarrow", "impliedby"], &[]),
    s("⇔", "if and only if", &["Leftrightarrow", "iff"], &["<=>"]),
    s("↑", "up arrow", &["uparrow"], &[]),
    s("↓", "down arrow", &["downarrow"], &[]),
    s("↦", "maps to", &["mapsto"], &["|->"]),
    // Superscripts and subscripts
    s("⁰", "superscript 0", &[], &["^0"]),
    s("¹", "superscript 1", &[], &["^1"]),
    s("²", "superscript 2", &[], &["^2"]),
    s("³", "superscript 3", &[], &["^3"]),
    s("⁴", "superscript 4", &[], &["^4"]),
    s("⁵", "superscript 5", &[], &["^5"]),
    s("⁶", "superscript 6", &[], &["^6"]),
    s("⁷", "superscript 7", &[], &["^7"]),
    s("⁸", "superscript 8", &[], &["^8"]),
    s("⁹", "superscript 9", &[], &["^9"]),
    s("⁺", "superscript plus", &[], &["^+"]),
    s("⁻", "superscript minus", &[], &["^-"]),
    s("ⁿ", "superscript n", &[], &["^n"]),
    s("₀", "subscript 0", &[], &["_0"]),
    s("₁", "subscript 1", &[], &["_1"]),
    s("₂", "subscript 2", &[], &["_2"]),
    s("₃", "subscript 3", &[], &["_3"]),
    s("₄", "subscript 4", &[], &["_4"]),
    s("₅", "subscript 5", &[], &["_5"]),
    s("₆", "subscript 6", &[], &["_6"]),
    s("₇", "subscript 7", &[], &["_7"]),
    s("₈", "subscript 8", &[], &["_8"]),
    s("₉", "subscript 9", &[], &["_9"]),
    s("½", "one half", &[], &["1/2"]),
    s("⅓", "one third", &[], &["1/3"]),
    s("¼", "one quarter", &[], &["1/4"]),
    s("¾", "three quarters", &[], &["3/4"]),
    // Units, currencies, marks
    s("µ", "micro sign", &[], &["u"]),
    s("‰", "per mille", &["permil"], &["o/oo"]),
    s("€", "euro", &["euro"], &["EUR"]),
    s("£", "pound", &["pounds"], &["GBP"]),
    s("¥", "yen", &["yen"], &["JPY"]),
    s("©", "copyright", &["copyright"], &["(c)"]),
    s("®", "registered", &["textregistered"], &["(r)"]),
    s("™", "trade mark", &["texttrademark"], &["(tm)"]),
    s("§", "section", &["S"], &[]),
    s("¶", "paragraph", &["P"], &[]),
    s("†", "dagger", &["dagger"], &["+"]),
    s("•", "bullet", &["bullet"], &["o"]),
    s("✓", "check mark", &["checkmark"], &["ok", "v"]),
    s("✗", "cross mark", &[], &["x"]),
    s("★", "star", &["bigstar"], &["*"]),
    // Typography
    s("–", "en dash", &["textendash"], &["--"]),
    s("—", "em dash", &["textemdash"], &["---"]),
    s("…", "ellipsis", &["dots", "ldots"], &["..."]),
    s("„", "double low quote", &[], &[",,"]),
    s("“", "left double quote", &[], &["``"]),
    s("”", "right double quote", &[], &["''"]),
    s("‚", "single low quote", &[], &[","]),
    s("‘", "left single quote", &[], &["`"]),
    s("’", "right single quote", &[], &["'"]),
    s("«", "left guillemet", &["guillemotleft"], &["<<"]),
    s("»", "right guillemet", &["guillemotright"], &[">>"]),
    s("\u{a0}", "no-break space", &["nobreakspace"], &["~"]),
];

/// The character a LaTeX name stands for (`alpha` → α, `le` → ≤), without the backslash.
pub fn latex(name: &str) -> Option<&'static str> {
    SYMBOLS.iter().find(|s| s.latex.contains(&name)).map(|s| s.ch)
}

/// LaTeX math (`\\sigma_d \\le f_{yd}`, the inside of `$…$`) as plain
/// Unicode text: names become their characters, `^2` and `_1` become
/// raised and lowered digits where Unicode has them, `\\frac{a}{b}` becomes
/// `a/b`, `\\sqrt{x}` becomes `√x`. What has no Unicode form stays readable
/// (`f_yd`).
pub fn latex_to_unicode(math: &str) -> String {
    let chars: Vec<char> = math.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // A group `{…}` or a single character at `i`: its text and the position after it.
    fn group(chars: &[char], i: usize) -> (String, usize) {
        match chars.get(i) {
            Some('{') => {
                let mut depth = 0;
                let mut j = i;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return (chars[i + 1..j].iter().collect(), j + 1);
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                (chars[i + 1..].iter().collect(), chars.len())
            }
            Some('\\') => {
                let n = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).count().max(1);
                (chars[i..(i + 1 + n).min(chars.len())].iter().collect(), (i + 1 + n).min(chars.len()))
            }
            Some(c) => (c.to_string(), i + 1),
            None => (String::new(), i),
        }
    }
    let wrap = |s: String| if s.chars().count() > 1 && s.contains([' ', '+', '-', '−', '·', '/']) { format!("({s})") } else { s };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let name: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
                if name.is_empty() {
                    // `\,` `\;` `\ ` are spaces, `\{` a brace.
                    match chars.get(i + 1) {
                        Some(',' | ';' | ' ' | ':' | '!') => out.push(' '),
                        Some(c) => out.push(*c),
                        None => {}
                    }
                    i += 2;
                    continue;
                }
                i += 1 + name.len();
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let (a, j) = group(&chars, i);
                        let (b, k) = group(&chars, j);
                        out.push_str(&format!("{}/{}", wrap(latex_to_unicode(&a)), wrap(latex_to_unicode(&b))));
                        i = k;
                    }
                    "sqrt" => {
                        let (a, j) = group(&chars, i);
                        out.push('√');
                        out.push_str(&wrap(latex_to_unicode(&a)));
                        i = j;
                    }
                    "text" | "mathrm" | "mathit" | "mathbf" | "operatorname" | "textrm" => {
                        let (a, j) = group(&chars, i);
                        out.push_str(&latex_to_unicode(&a));
                        i = j;
                    }
                    "left" | "right" | "displaystyle" => {}
                    "quad" | "qquad" => out.push_str("  "),
                    _ => match latex(&name) {
                        Some(ch) => out.push_str(ch),
                        None => out.push_str(&format!("\\{name}")),
                    },
                }
            }
            '^' | '_' => {
                let (g, j) = group(&chars, i + 1);
                let inner = latex_to_unicode(&g);
                let table: &[(char, char)] = if c == '^' { &SUPERSCRIPTS } else { &SUBSCRIPTS };
                let mapped: Option<String> = inner.chars().map(|x| table.iter().find(|(a, _)| *a == x).map(|(_, b)| *b)).collect();
                match mapped {
                    Some(m) if !m.is_empty() => out.push_str(&m),
                    _ if c == '^' => out.push_str(&format!("^{}", wrap(inner))),
                    _ => out.push_str(&format!("_{inner}")),
                }
                i = j;
            }
            '{' | '}' => i += 1,
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

const SUPERSCRIPTS: [(char, char); 16] = [
    ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'),
    ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('−', '⁻'), ('n', 'ⁿ'), ('i', 'ⁱ'), ('=', '⁼'),
];
const SUBSCRIPTS: [(char, char); 13] = [
    ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'),
    ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'),
];

/// The `$…$` pieces of a line of Markdown text as Unicode
/// ([`latex_to_unicode`]); as in GitHub's Markdown a `$` opens only before
/// a non-space and closes only after one, not before a digit – `$5 and $10`
/// stays text. `\\$` is a dollar sign.
pub fn inline_math(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'$') {
            out.push('$');
            i += 2;
            continue;
        }
        if chars[i] == '$' && chars.get(i + 1).is_some_and(|c| !c.is_whitespace() && *c != '$') {
            let close = (i + 2..chars.len()).find(|&j| {
                chars[j] == '$' && !chars[j - 1].is_whitespace() && chars[j - 1] != '\\' && !chars.get(j + 1).is_some_and(char::is_ascii_digit)
            });
            if let Some(j) = close {
                let math: String = chars[i + 1..j].iter().collect();
                out.push_str(&latex_to_unicode(&math));
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// How well `sym` matches the typed `query`: lower is better, `None` not at all.
fn rank(sym: &Symbol, query: &str) -> Option<u8> {
    let q = query.strip_prefix('\\').unwrap_or(query);
    let lower = q.to_lowercase();
    let latex = query.starts_with('\\');
    if sym.ch == query || sym.ascii.contains(&query) || sym.latex.contains(&q) {
        return Some(0);
    }
    if latex {
        // A backslash asks for LaTeX names only.
        return sym.latex.iter().any(|l| l.starts_with(q)).then_some(2);
    }
    if sym.name == q {
        return Some(1);
    }
    if sym.name.to_lowercase() == lower {
        return Some(2);
    }
    if sym.name.starts_with(q) {
        return Some(3);
    }
    if sym.name.to_lowercase().starts_with(&lower) || sym.latex.iter().any(|l| l.starts_with(q)) {
        return Some(4);
    }
    if sym.ascii.iter().any(|a| a.starts_with(query)) {
        return Some(5);
    }
    // A word of the name: "equal" finds ≤ ≥ ≠ ≈.
    if sym.name.to_lowercase().split([' ', '(', ')']).any(|w| !w.is_empty() && w.starts_with(&lower)) {
        return Some(6);
    }
    None
}

/// The characters matching `query`, best first; the `recent` ones lead
/// among equally good matches (all of them for an empty query).
pub fn search(query: &str, recent: &[String]) -> Vec<&'static Symbol> {
    let at = |s: &Symbol| recent.iter().position(|r| r == s.ch).unwrap_or(usize::MAX);
    let mut found: Vec<(u8, usize, usize, &'static Symbol)> = SYMBOLS
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let r = if query.is_empty() { Some(0) } else { rank(s, query) };
            r.map(|r| (r, at(s), i, s))
        })
        .collect();
    found.sort_by_key(|&(r, recent, i, _)| (r, recent, i));
    found.into_iter().map(|(.., s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(q: &str) -> &'static str {
        search(q, &[])[0].ch
    }

    #[test]
    fn found_by_name_latex_ascii_and_itself() {
        assert_eq!(first("Delta"), "Δ");
        assert_eq!(first("delta"), "δ");
        assert_eq!(first("Del"), "Δ");
        assert_eq!(first("\\le"), "≤");
        assert_eq!(first("<="), "≤");
        assert_eq!(first("<>"), "≠");
        assert_eq!(first("=>"), "⇒");
        assert_eq!(first("->"), "→");
        assert_eq!(first("≈"), "≈");
        assert_eq!(first("^2"), "²");
        assert!(search("equal", &[]).iter().any(|s| s.ch == "≠"));
        assert!(search("\\zzz", &[]).is_empty());
    }

    #[test]
    fn recent_ones_lead() {
        let recent = vec!["Ω".to_string()];
        assert_eq!(search("", &recent)[0].ch, "Ω");
        assert_eq!(search("", &[])[0].ch, "α");
    }

    #[test]
    fn latex_names() {
        assert_eq!(latex("alpha"), Some("α"));
        assert_eq!(latex("varphi"), Some("φ"));
        assert_eq!(latex("leq"), Some("≤"));
        assert_eq!(latex("nope"), None);
    }

    #[test]
    fn latex_as_unicode() {
        assert_eq!(latex_to_unicode("\\sigma_d \\le f_{yd}"), "σ_d ≤ f_yd");
        assert_eq!(latex_to_unicode("A = \\pi r^2"), "A = π r²");
        assert_eq!(latex_to_unicode("x_{12}^{-1}"), "x₁₂⁻¹");
        assert_eq!(latex_to_unicode("\\frac{M}{W_y}"), "M/W_y");
        assert_eq!(latex_to_unicode("\\frac{a+b}{2}"), "(a+b)/2");
        assert_eq!(latex_to_unicode("\\sqrt{h^2 + l^2}"), "√(h² + l²)");
        assert_eq!(latex_to_unicode("e^{x}"), "e^x");
        assert_eq!(latex_to_unicode("5\\,\\text{kN}"), "5 kN");
        assert_eq!(latex_to_unicode("\\unknown"), "\\unknown");
    }

    #[test]
    fn math_between_dollars() {
        assert_eq!(inline_math("with $\\gamma_M = 1.1$ and $\\eta \\le 1$."), "with γ_M = 1.1 and η ≤ 1.");
        assert_eq!(inline_math("costs $5 and $10 each"), "costs $5 and $10 each");
        assert_eq!(inline_math("a $ b $ c"), "a $ b $ c");
        assert_eq!(inline_math("\\$\\alpha$"), "$\\alpha$");
        assert_eq!(inline_math("no math"), "no math");
    }

    #[test]
    fn no_character_twice() {
        for (i, a) in SYMBOLS.iter().enumerate() {
            assert!(!SYMBOLS[i + 1..].iter().any(|b| b.ch == a.ch), "{}", a.ch);
        }
    }
}
