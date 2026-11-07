//! Formula typesetting: LaTeX and Typst math written from the syntax tree.
//!
//! LaTeX feeds the 2D rendering in the terminal (term-maths), Typst the
//! calculation report. Both come from the same [`Expr`], so all outputs show
//! the same formula.

use super::expr::{Expr, Op, Rel};
use super::{verdict, Mode, Out, Values};
use crate::quantity::{format_sig, pretty, Quantity};

/// Target language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lang {
    Latex,
    Typst,
}

const GREEK_LATEX: &[(char, &str)] = &[
    ('α', "alpha"),
    ('β', "beta"),
    ('γ', "gamma"),
    ('δ', "delta"),
    ('ε', "varepsilon"),
    ('ζ', "zeta"),
    ('η', "eta"),
    ('θ', "theta"),
    ('κ', "kappa"),
    ('λ', "lambda"),
    ('μ', "mu"),
    ('ν', "nu"),
    ('ξ', "xi"),
    ('π', "pi"),
    ('ρ', "rho"),
    ('σ', "sigma"),
    ('τ', "tau"),
    ('φ', "varphi"),
    ('χ', "chi"),
    ('ψ', "psi"),
    ('ω', "omega"),
    ('Γ', "Gamma"),
    ('Δ', "Delta"),
    ('Θ', "Theta"),
    ('Λ', "Lambda"),
    ('Σ', "Sigma"),
    ('Φ', "Phi"),
    ('Ψ', "Psi"),
    ('Ω', "Omega"),
];

fn greek_name(c: char) -> Option<&'static str> {
    GREEK_LATEX.iter().find(|(g, _)| *g == c).map(|(_, n)| *n)
}

/// Text inside quotes / `\text{}`, escaped.
fn text(lang: Lang, s: &str) -> String {
    match lang {
        Lang::Latex => {
            let esc: String = s
                .chars()
                .map(|c| match c {
                    '\\' => "\\textbackslash{}".to_string(),
                    '{' | '}' | '%' | '&' | '#' | '_' | '$' => format!("\\{c}"),
                    c => c.to_string(),
                })
                .collect();
            format!("\\text{{{esc}}}")
        }
        Lang::Typst => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

/// The base of a name: one letter (Greek by name), else upright text.
fn base(lang: Lang, b: &str) -> String {
    let mut chars = b.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => match (greek_name(c), lang) {
            (Some(g), Lang::Latex) => format!("\\{g}"),
            (Some(g), Lang::Typst) => g.to_string(),
            (None, _) => c.to_string(),
        },
        _ => match lang {
            Lang::Latex => format!("\\mathit{{{b}}}"),
            Lang::Typst => format!("italic({})", text(lang, b)),
        },
    }
}

/// `f_tk` → `f_{\mathrm{tk}}` / `f_"tk"`: the index set upright and lowered.
pub fn name(lang: Lang, n: &str) -> String {
    match n.split_once('_') {
        Some((b, idx)) if !b.is_empty() && !idx.is_empty() => {
            let idx = idx.replace('_', ",");
            match lang {
                Lang::Latex => format!("{}_{{\\mathrm{{{idx}}}}}", base(lang, b)),
                Lang::Typst => format!("{}_{}", base(lang, b), text(lang, &idx)),
            }
        }
        _ => base(lang, n),
    }
}

/// A number as the display shows it (decimal point or comma); LaTeX needs
/// `{,}` so the comma does not space.
pub fn number(lang: Lang, s: &str) -> String {
    let s = &crate::locale::show(&s.replace(',', "."));
    match lang {
        Lang::Latex => s.replace(',', "{,}"),
        Lang::Typst if s.contains(',') => text(lang, s),
        Lang::Typst => s.to_string(),
    }
}

/// A unit after a value: thin space and the unit upright.
pub fn unit(lang: Lang, u: &str) -> String {
    match lang {
        Lang::Latex if u == "°" => "{°}".to_string(),
        Lang::Typst if u == "°" => "°".to_string(),
        Lang::Latex => format!("\\,{}", text(lang, u)),
        Lang::Typst => format!(" thin {}", text(lang, u)),
    }
}

/// A quantity in its unit, 3 significant digits.
pub fn quantity(lang: Lang, q: &Quantity) -> String {
    let (v, u) = q.shown();
    let n = number(lang, &format_sig(v, 3, true));
    match u {
        Some(u) => format!("{n}{}", unit(lang, &pretty(&u))),
        None => n,
    }
}

fn paren(lang: Lang, s: String) -> String {
    match lang {
        Lang::Latex => format!("\\left({s}\\right)"),
        Lang::Typst => format!("({s})"),
    }
}

/// Writes an expression; `leaf` may replace variables and grid references
/// (by their values, for the substituted step).
pub fn expr(lang: Lang, e: &Expr, leaf: &dyn Fn(&Expr) -> Option<String>) -> String {
    fn prec(e: &Expr) -> u8 {
        match e {
            Expr::Bin(Op::Add | Op::Sub, ..) => 1,
            // A fraction is drawn as a unit: it needs no parentheses.
            Expr::Bin(Op::Div, ..) => 4,
            Expr::Bin(Op::Mul, ..) | Expr::Neg(_) => 2,
            Expr::Bin(Op::Pow, ..) => 3,
            _ => 5,
        }
    }
    fn go(lang: Lang, e: &Expr, leaf: &dyn Fn(&Expr) -> Option<String>, need: u8) -> String {
        if let Some(s) = leaf(e) {
            return s;
        }
        let s = match e {
            Expr::Num { text: t, unit: u, .. } => match u {
                Some(u) => format!("{}{}", number(lang, t), unit(lang, &pretty(u))),
                None => number(lang, t),
            },
            Expr::Var(n) => name(lang, n),
            Expr::Cell(r) => text(lang, r),
            Expr::Neg(x) => format!("-{}", go(lang, x, leaf, 3)),
            Expr::Bin(Op::Div, a, b) => match lang {
                Lang::Latex => format!("\\frac{{{}}}{{{}}}", go(lang, a, leaf, 0), go(lang, b, leaf, 0)),
                Lang::Typst => format!("({})/({})", go(lang, a, leaf, 0), go(lang, b, leaf, 0)),
            },
            Expr::Bin(Op::Pow, a, b) => match lang {
                Lang::Latex => format!("{{{}}}^{{{}}}", go(lang, a, leaf, 4), go(lang, b, leaf, 0)),
                Lang::Typst => format!("{}^({})", go(lang, a, leaf, 4), go(lang, b, leaf, 0)),
            },
            Expr::Bin(op, a, b) => {
                let p = prec(e);
                let sym = match (op, lang) {
                    (Op::Add, _) => " + ",
                    (Op::Sub, _) => " - ",
                    (_, Lang::Latex) => " \\cdot ",
                    (_, Lang::Typst) => " dot ",
                };
                format!("{}{sym}{}", go(lang, a, leaf, p), go(lang, b, leaf, p + 1))
            }
            Expr::Call(f, args) if f == "sqrt" || f == "wurzel" => {
                let inner = args.iter().map(|a| go(lang, a, leaf, 0)).collect::<Vec<_>>().join("; ");
                match lang {
                    Lang::Latex => format!("\\sqrt{{{inner}}}"),
                    Lang::Typst => format!("sqrt({inner})"),
                }
            }
            Expr::Call(f, args) => {
                let sep = match lang {
                    Lang::Latex => ";\\, ",
                    Lang::Typst => "; ",
                };
                let inner = args.iter().map(|a| go(lang, a, leaf, 0)).collect::<Vec<_>>().join(sep);
                let fname = match lang {
                    Lang::Latex => format!("\\operatorname{{{f}}}"),
                    Lang::Typst => format!("op(\"{f}\")"),
                };
                return format!("{fname}{}", paren(lang, inner));
            }
        };
        if prec(e) < need { paren(lang, s) } else { s }
    }
    go(lang, e, leaf, 0)
}

fn rel(lang: Lang, r: Rel) -> &'static str {
    match (r, lang) {
        (Rel::Le, Lang::Latex) => "\\le",
        (Rel::Ge, Lang::Latex) => "\\ge",
        (Rel::Le, Lang::Typst) => "<=",
        (Rel::Ge, Lang::Typst) => ">=",
        (Rel::Lt, _) => "<",
        (Rel::Gt, _) => ">",
    }
}

/// The steps of a line: formula, substituted values, result.
fn steps(lang: Lang, e: &Expr, values: &Values, q: &Quantity, mode: Mode) -> Vec<String> {
    let result = quantity(lang, q);
    // An input in its own unit is shown as written: 0,60 kN/m².
    let literal = match e {
        Expr::Num { unit, .. } => Some(unit),
        Expr::Neg(x) => match &**x {
            Expr::Num { unit, .. } => Some(unit),
            _ => None,
        },
        _ => None,
    };
    if let Some(u) = literal {
        let same = match u {
            Some(u) => q.display.as_ref() == Some(u),
            None => q.dim.is_none() && q.display.is_none(),
        };
        return vec![if same { expr(lang, e, &|_| None) } else { result }];
    }
    let mut out = Vec::new();
    if mode != Mode::Result {
        out.push(expr(lang, e, &|_| None));
        if mode == Mode::Full && !matches!(e, Expr::Var(_) | Expr::Cell(_)) {
            let s = expr(lang, e, &|leaf| match leaf {
                Expr::Var(n) | Expr::Cell(n) => values.get(n).map(|v| number(lang, v)),
                Expr::Num { text: t, .. } => Some(number(lang, t)),
                _ => None,
            });
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out.push(result);
    out
}

/// One evaluated line as math (`None` for headings, comments, blanks and
/// lines that could not be calculated).
pub fn line(lang: Lang, o: &Out) -> Option<String> {
    match o {
        Out::Assign { name: n, expr: e, values, result: Ok(q), mode, .. } => Some(format!("{} = {}", name(lang, n), steps(lang, e, values, q, *mode).join(" = "))),
        Out::Show { expr: e, values, result: Ok(q), mode, .. } => Some(steps(lang, e, values, q, *mode).join(" = ")),
        Out::Check { lhs, rel: r, rhs, values, sides: Ok((l, rq)), mode, .. } => {
            let m = if *mode == Mode::Full { Mode::Formula } else { *mode };
            let v = verdict(*r, l, rq);
            let (space, eta, mark) = match lang {
                Lang::Latex => ("\\quad ", "\\eta", if v.ok { "\\checkmark" } else { "\\times" }),
                Lang::Typst => ("quad ", "eta", if v.ok { "checkmark" } else { "crossmark" }),
            };
            let eta = match v.eta {
                Some(x) => format!(" {space}{eta} = {}", number(lang, &format!("{x:.2}"))),
                None => String::new(),
            };
            Some(format!(
                "{} {} {}{eta} {space}{mark}",
                steps(lang, lhs, values, l, m).join(" = "),
                rel(lang, *r),
                steps(lang, rhs, values, rq, m).join(" = ")
            ))
        }
        _ => None,
    }
}

/// The language of the report text (hyphenation, quotes): the user's locale
/// from LC_ALL, LC_MESSAGES or LANG, English without one.
fn report_lang() -> String {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    let code: String = lang.unwrap_or_default().chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
    if code.len() == 2 { code } else { "en".to_string() }
}

/// The calculation as a Typst document: headings, comments as text, each
/// line as display math. `title` heads the report.
pub fn typst_report(title: &str, out: &[Out]) -> String {
    let mut s = String::new();
    s.push_str("#set page(paper: \"a4\", margin: 2cm)\n");
    s.push_str(&format!("#set text(lang: \"{}\", size: 10.5pt)\n", report_lang()));
    s.push_str("#set math.equation(numbering: none)\n");
    s.push_str("#show math.equation.where(block: true): set align(left)\n\n");
    if !title.is_empty() {
        s.push_str(&format!("#align(center, text(16pt, weight: \"bold\")[{}])\n\n", escape_markup(title)));
    }
    for o in out {
        match o {
            Out::Blank => s.push('\n'),
            Out::Heading { level, text: t } => s.push_str(&format!("{} {}\n\n", "=".repeat(*level), escape_markup(t))),
            Out::Comment(c) => s.push_str(&format!("{}\n\n", escape_markup(c))),
            Out::Error(e) => s.push_str(&format!("#text(fill: red)[{}]\n\n", escape_markup(e))),
            _ => {
                let comment = match o {
                    Out::Assign { comment, .. } | Out::Show { comment, .. } | Out::Check { comment, .. } => comment.clone(),
                    _ => None,
                };
                match line(Lang::Typst, o) {
                    Some(m) => {
                        let ok = match o {
                            Out::Check { rel: r, sides: Ok((l, rq)), .. } => Some(verdict(*r, l, rq).ok),
                            _ => None,
                        };
                        let m = match ok {
                            Some(false) => format!("#text(fill: red)[$ {m} $]"),
                            _ => format!("$ {m} $"),
                        };
                        match comment {
                            Some(c) if !c.is_empty() => {
                                s.push_str(&format!("#grid(columns: (1fr, auto), gutter: 1em, {m}, align(horizon, text(9pt, style: \"italic\")[{}]))\n\n", escape_markup(&c)))
                            }
                            _ => s.push_str(&format!("{m}\n\n")),
                        }
                    }
                    None => {
                        let msg = super::problems(std::slice::from_ref(o)).join("; ");
                        s.push_str(&format!("#text(fill: red)[{}]\n\n", escape_markup(&msg)));
                    }
                }
            }
        }
    }
    s
}

/// Text for Typst markup: characters with a meaning there are escaped.
fn escape_markup(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(c, '\\' | '#' | '$' | '*' | '_' | '[' | ']' | '<' | '>' | '@' | '`' | '=' | '-' | '+' | '/' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::evaluate;

    fn outs(src: &str) -> Vec<Out> {
        evaluate(&src.lines().map(str::to_string).collect::<Vec<_>>(), None)
    }

    #[test]
    fn names_with_indices_and_greek() {
        assert_eq!(name(Lang::Latex, "f_tk"), "f_{\\mathrm{tk}}");
        assert_eq!(name(Lang::Latex, "γ_M"), "\\gamma_{\\mathrm{M}}");
        assert_eq!(name(Lang::Typst, "γ_M"), "gamma_\"M\"");
        assert_eq!(name(Lang::Typst, "Area"), "italic(\"Area\")");
    }

    #[test]
    fn latex_lines() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let o = outs("f_tk = 84 kN/m\nγ_M = 1,4\nf_d = f_tk / γ_M\nn = 38,2 kN/m\nn <= f_d");
        assert_eq!(line(Lang::Latex, &o[0]).unwrap(), "f_{\\mathrm{tk}} = 84\\,\\text{kN/m}");
        assert_eq!(
            line(Lang::Latex, &o[2]).unwrap(),
            "f_{\\mathrm{d}} = \\frac{f_{\\mathrm{tk}}}{\\gamma_{\\mathrm{M}}} = \\frac{84}{1{,}4} = 60{,}0\\,\\text{kN/m}"
        );
        assert_eq!(line(Lang::Latex, &o[4]).unwrap(), "n = 38{,}2\\,\\text{kN/m} \\le f_{\\mathrm{d}} = 60{,}0\\,\\text{kN/m} \\quad \\eta = 0{,}64 \\quad \\checkmark");
    }

    #[test]
    fn parentheses_and_functions() {
        let o = outs("a = 1\nb = 2\nx = (a + b)·max(a; b)^2 - sqrt(a)\ny = (a + b)/(a - b)");
        assert_eq!(
            line(Lang::Latex, &o[2]).unwrap().split(" = ").nth(1).unwrap(),
            "\\left(a + b\\right) \\cdot {\\operatorname{max}\\left(a;\\, b\\right)}^{2} - \\sqrt{a}"
        );
        assert_eq!(line(Lang::Typst, &o[3]).unwrap().split(" = ").nth(1).unwrap(), "(a + b)/(a - b)");
    }

    #[test]
    fn typst_report_has_math_and_headings() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let o = outs("## Check\nq = 0,60 kN/m²   ' snow\nq <= 1 kN/m²");
        let r = typst_report("Beam", &o);
        assert!(r.contains("== Check"), "{r}");
        assert!(r.contains("$ q = \"0,60\" thin \"kN/m²\" $"), "{r}");
        assert!(r.contains("checkmark"), "{r}");
        assert!(r.contains("[snow]"), "{r}");
    }
}
