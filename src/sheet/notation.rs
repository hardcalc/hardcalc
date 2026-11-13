//! Formula typesetting: LaTeX and Typst math written from the syntax tree.
//!
//! LaTeX feeds the 2D rendering in the terminal (term-maths), Typst the
//! calculation report. Both come from the same [`Expr`], so all outputs show
//! the same formula.

use super::expr::{self, Expr, Op, Rel};
use super::{verdict, Mode, Out, Series, Values};
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

/// The name of an own function before its brackets (`M_ss` → M with the
/// index ss); Typst needs a space, or the brackets join the index.
fn own_function(lang: Lang, f: &str) -> String {
    match lang {
        Lang::Latex => name(lang, f),
        Lang::Typst => format!("{} ", name(lang, f)),
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
            Expr::Call(f, args) if expr::is_integral(f, args) => {
                let (x, a, b, body) = (go(lang, &args[1], leaf, 0), go(lang, &args[2], leaf, 0), go(lang, &args[3], leaf, 0), go(lang, &args[0], leaf, 2));
                return match lang {
                    Lang::Latex => format!("\\int_{{{a}}}^{{{b}}} {body} \\,\\mathrm{{d}}{x}"),
                    Lang::Typst => format!("integral_({a})^({b}) {body} dif {x}"),
                };
            }
            Expr::Call(f, args) if expr::is_derivative(f, args) => {
                let (x, at, body) = (go(lang, &args[1], leaf, 0), go(lang, &args[2], leaf, 0), go(lang, &args[0], leaf, 0));
                return match lang {
                    Lang::Latex => format!("\\left.\\frac{{\\mathrm{{d}}}}{{\\mathrm{{d}}{x}}} \\left({body}\\right)\\right|_{{{x}={at}}}"),
                    Lang::Typst => format!("lr((dif)/(dif {x}) ({body}) |)_({x} = {at})"),
                };
            }
            Expr::Range { items, unit: u } => {
                let dots = match lang {
                    Lang::Latex => "\\dots",
                    Lang::Typst => "dots",
                };
                let parts: Vec<String> = items.iter().map(|i| i.as_ref().map_or(dots.to_string(), |e| go(lang, e, leaf, 0))).collect();
                let u = u.as_ref().map(|u| unit(lang, &pretty(u))).unwrap_or_default();
                return match lang {
                    Lang::Latex => format!("\\{{{}\\}}{u}", parts.join(";\\, ")),
                    Lang::Typst => format!("{{{}}}{u}", parts.join("; ")),
                };
            }
            Expr::Call(f, args) if f.eq_ignore_ascii_case("sqrt") || f.eq_ignore_ascii_case("wurzel") => {
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
                // Built-in functions upright; own ones like variables (M_ss → M with index ss).
                let fname = match lang {
                    // In Typst a bracket right after an index would join it.
                    _ if !super::FUNCTIONS.contains(&f.to_lowercase().as_str()) => own_function(lang, f),
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
fn steps(lang: Lang, e: &Expr, values: &Values, q: &Quantity, mode: Mode, series: Option<&Series>) -> Vec<String> {
    let result = match series.filter(|s| s.items.len() > 1).and_then(Series::span) {
        // A range: its smallest and largest value, how many.
        Some((lo, hi)) => {
            let (dots, space) = match lang {
                Lang::Latex => ("\\dots", "\\quad"),
                Lang::Typst => ("dots", "quad"),
            };
            format!("{} {dots} {} {space} ({})", number(lang, &format_sig(lo, 3, true)), quantity(lang, hi), series.map_or(0, |s| s.items.len()))
        }
        None => quantity(lang, q),
    };
    let mode = if series.is_some() && mode == Mode::Full { Mode::Formula } else { mode };
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
        Out::Assign { name: n, expr: e, values, result: Ok(q), series, mode, .. } => Some(format!("{} = {}", name(lang, n), steps(lang, e, values, q, *mode, series.as_ref()).join(" = "))),
        Out::Show { expr: e, values, result: Ok(q), series, mode, .. } => Some(steps(lang, e, values, q, *mode, series.as_ref()).join(" = ")),
        Out::Def { name: n, params, expr: e, .. } => {
            let params: Vec<String> = params.iter().map(|p| name(lang, p)).collect();
            Some(format!("{}{} = {}", own_function(lang, n), paren(lang, params.join("; ")), expr(lang, e, &|_| None)))
        }
        Out::Check { lhs, rel: r, rhs, values, sides: Ok((l, rq)), at, mode, .. } => {
            let m = if *mode == Mode::Full { Mode::Formula } else { *mode };
            let v = verdict(*r, l, rq);
            let (space, eta, mark) = match lang {
                Lang::Latex => ("\\quad ", "\\eta", if v.ok { "\\checkmark" } else { "\\times" }),
                Lang::Typst => ("quad ", "eta", if v.ok { "checkmark" } else { "crossmark" }),
            };
            let max = match (at.is_some(), lang) {
                (false, _) => "",
                (true, Lang::Latex) => "\\max ",
                (true, Lang::Typst) => "max ",
            };
            let mut eta = match v.eta {
                Some(x) => format!(" {space}{max}{eta} = {}", number(lang, &format!("{x:.2}"))),
                None => String::new(),
            };
            // Over a range: where it is decisive.
            if let Some(super::At { over: Some(o), x: Some(x), .. }) = at {
                let word = match lang {
                    Lang::Latex => "\\text{ at }",
                    Lang::Typst => "\" at \"",
                };
                eta.push_str(&format!("{word}{} = {}", name(lang, o), quantity(lang, x)));
            }
            Some(format!(
                "{} {} {}{eta} {space}{mark}",
                steps(lang, lhs, values, l, m, None).join(" = "),
                rel(lang, *r),
                steps(lang, rhs, values, rq, m, None).join(" = ")
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
    // With any label, every formula gets a label column of one width, so
    // the formulas start at the same place.
    let label_w = out.iter().filter_map(|o| o.label()).map(|l| l.chars().count()).max();
    let label_col = label_w.map(|n| format!("{:.1}em", (n as f64 * 0.55 + 0.5).clamp(2.0, 14.0)));
    for o in out {
        match o {
            Out::Blank => s.push('\n'),
            Out::Heading { level, text: t } => s.push_str(&format!("{} {}\n\n", "=".repeat(*level), escape_markup(t))),
            Out::Comment(c) => s.push_str(&format!("{}\n\n", escape_markup(c))),
            Out::Error(e) => s.push_str(&format!("#text(fill: red)[{}]\n\n", escape_markup(e))),
            _ => {
                let body = match (o, line(Lang::Typst, o)) {
                    (Out::Plot { name, plot: Ok(p), .. }, _) => Some(typst_plot(name, p)),
                    (_, Some(m)) => {
                        let ok = match o {
                            Out::Check { rel: r, sides: Ok((l, rq)), .. } => Some(verdict(*r, l, rq).ok),
                            _ => None,
                        };
                        Some(match ok {
                            Some(false) => format!("text(fill: red)[$ {m} $]"),
                            _ => format!("[$ {m} $]"),
                        })
                    }
                    _ => None,
                };
                let Some(body) = body else {
                    let msg = super::problems(std::slice::from_ref(o)).join("; ");
                    s.push_str(&format!("#text(fill: red)[{}]\n\n", escape_markup(&msg)));
                    continue;
                };
                // The label left of the formula, the comment right of it.
                let label = o.label().map(|l| format!("align(horizon, text(9pt, weight: \"bold\")[{}])", escape_markup(l)));
                let comment = o.comment().map(|c| format!("align(horizon, text(9pt, style: \"italic\")[{}])", escape_markup(c)));
                let (mut columns, mut cells) = (Vec::new(), Vec::new());
                if let Some(w) = &label_col {
                    columns.push(w.clone());
                    cells.push(label.unwrap_or_else(|| "[]".into()));
                }
                columns.push("1fr".into());
                // A comment beside a long formula would run into it: it goes below.
                let long = super::Render::default().body(o).chars().count() + o.comment().map_or(0, |c| c.chars().count()) > REPORT_LINE;
                match comment {
                    Some(c) if long || matches!(o, Out::Plot { .. }) => cells.push(format!("stack(spacing: 0.4em, {body}, align(right, {c}))")),
                    Some(c) => {
                        cells.push(body);
                        columns.push("auto".into());
                        cells.push(c);
                    }
                    None => cells.push(body),
                }
                s.push_str(&format!("#grid(columns: ({},), gutter: 1em, {})\n\n", columns.join(", "), cells.join(", ")));
            }
        }
    }
    s
}

/// Characters of formula and comment that fit on one line of the report
/// beside each other.
const REPORT_LINE: usize = 80;

/// A plot drawn with Typst: the curves as lines in a frame, the ranges of
/// both axes written at its edges.
fn typst_plot(name: &str, p: &super::Plot) -> String {
    const W: f64 = 12.0;
    const H: f64 = 5.0;
    let data = match crate::chart::from_plot(name, p, false) {
        Ok(d) => d,
        Err(e) => return format!("text(fill: red)[{}]", escape_markup(&format!("{name}: {e}"))),
    };
    let values: Vec<f64> = data.series.iter().flat_map(|s| s.values.iter().flatten().copied()).collect();
    let (mut lo, mut hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    // The zero line belongs in the picture.
    lo = lo.min(0.0);
    hi = hi.max(0.0);
    if hi - lo < 1e-12 {
        hi = lo + 1.0;
    }
    let n = data.categories.len().max(2);
    let xs: Vec<f64> = match &data.xs {
        Some(xs) => xs.clone(),
        None => (0..n).map(|i| i as f64).collect(),
    };
    let (x0, x1) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = if x1 - x0 < 1e-12 { 1.0 } else { x1 - x0 };
    let px = |x: f64| (x - x0) / span * W;
    let py = |y: f64| H - (y - lo) / (hi - lo) * H;
    let colors = ["blue", "red", "green", "orange"];
    let mut shapes = Vec::new();
    // The axis at zero.
    shapes.push(format!("place(line(start: (0cm, {:.3}cm), end: ({W}cm, {:.3}cm), stroke: 0.4pt + gray))", py(0.0), py(0.0)));
    for (k, s) in data.series.iter().enumerate() {
        let points: Vec<String> = s.values.iter().zip(&xs).filter_map(|(v, x)| v.map(|v| format!("({:.3}cm, {:.3}cm)", px(*x), py(v)))).collect();
        if points.len() > 1 {
            shapes.push(format!("place(path(stroke: 1pt + {}, {}))", colors[k % colors.len()], points.join(", ")));
        }
    }
    let unit = super::unit_name_text(&data.unit);
    let x_unit = super::unit_name_text(&data.x_unit);
    let fmt = |v: f64| format_sig(v, 3, false);
    let x_name = p.x.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
    let curves: Vec<String> = data.series.iter().enumerate().map(|(k, s)| format!("text(fill: {})[{}]", colors[k % colors.len()], escape_markup(&s.name))).collect();
    format!(
        "[#text(9pt, weight: \"bold\")[{}] #h(1em) #{} \\ #box(width: {W}cm, height: {H}cm, stroke: 0.5pt + gray)[#{}] \\ #text(8pt)[{} … {}{} · {} = {} … {}{}]]",
        escape_markup(&data.title),
        curves.join(" #h(0.6em) #"),
        shapes.join(" #"),
        fmt(lo),
        fmt(hi),
        unit,
        escape_markup(&x_name),
        fmt(x0),
        fmt(x1),
        x_unit,
    )
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
