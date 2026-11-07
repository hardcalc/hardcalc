//! Calculation sheets: engineering calculations with units, readable
//! formulas and the way to the result, in ```hardcalc blocks.
//!
//! ````markdown
//! ```hardcalc
//! ## Bending check
//! f_yk = 235 N/mm²        ' yield strength
//! γ_M  = 1,1
//! f_yd = f_yk / γ_M
//! σ_Ed = Loads!C12       ' value from the table
//! σ_Ed ≤ f_yd
//! ```
//! ````
//!
//! All blocks of a document form one sheet, evaluated top to bottom; grid
//! references are read from the document's tables. The syntax tree
//! ([`expr::Expr`]) is the only truth: text, LaTeX and Typst are written
//! from it.

pub mod expr;
pub mod notation;

use crate::engine::Workbook;
use crate::model::{Block, Document};
use crate::quantity::{format_sig, pretty, Quantity};
use crate::value::Value;
use expr::{to_text, Expr, Line, Mode, Op, Rel, Stmt, TextStyle};
use std::collections::HashMap;

/// Info string of the fenced block that holds a calculation sheet.
pub const INFO: &str = "hardcalc";

/// The lines of all ```hardcalc blocks of a document, in order.
pub fn source_lines(doc: &Document) -> Vec<String> {
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Code { info, body } if is_sheet_info(info) => Some(body.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

pub fn is_sheet_info(info: &str) -> bool {
    info.split_whitespace().next() == Some(INFO)
}

/// A variable: its value and, for an input, the number as written (shown
/// in the substituted step: `0,60` rather than `0,600`).
#[derive(Debug, Clone)]
struct Var {
    q: Quantity,
    written: Option<String>,
}

/// One evaluated line.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Blank,
    Heading { level: usize, text: String },
    Comment(String),
    Assign { name: String, expr: Expr, values: Values, result: Result<Quantity, String>, mode: Mode, comment: Option<String> },
    Check { lhs: Expr, rel: Rel, rhs: Expr, values: Values, sides: Result<(Quantity, Quantity), String>, mode: Mode, comment: Option<String> },
    Show { expr: Expr, values: Values, result: Result<Quantity, String>, mode: Mode, comment: Option<String> },
    Error(String),
}

/// The shown numbers of the variables and grid references of a line.
pub type Values = HashMap<String, String>;

/// Result of a check line: utilisation η and whether it holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    pub eta: Option<f64>,
    pub ok: bool,
}

pub fn verdict(rel: Rel, l: &Quantity, r: &Quantity) -> Verdict {
    let ok = match rel {
        Rel::Le => l.value <= r.value,
        Rel::Lt => l.value < r.value,
        Rel::Ge => l.value >= r.value,
        Rel::Gt => l.value > r.value,
    };
    let (num, den) = if rel.upper_bound() { (l.value, r.value) } else { (r.value, l.value) };
    let eta = (den != 0.0).then(|| (num / den).abs());
    Verdict { eta, ok }
}

/// Evaluates a sheet. `wb` resolves grid references (`Loads!C12`).
pub fn evaluate(lines: &[String], mut wb: Option<&mut Workbook>) -> Vec<Out> {
    let mut vars: HashMap<String, Var> = HashMap::new();
    let mut mode = Mode::default();
    let mut out = Vec::new();
    for src in lines {
        let line = expr::parse_line(src);
        let o = match line {
            Line::Blank => Out::Blank,
            Line::Heading { level, text } => Out::Heading { level, text },
            Line::Mode(m) => {
                mode = m;
                continue;
            }
            Line::Error { msg } => Out::Error(format!("{}: {msg}", src.trim())),
            Line::Stmt { stmt: None, comment, .. } => Out::Comment(comment.unwrap_or_default()),
            Line::Stmt { stmt: Some(stmt), comment, mode: own } => {
                let mode = own.unwrap_or(mode);
                let mut ctx = Ctx { vars: &vars, wb: wb.as_deref_mut(), values: Values::new() };
                match stmt {
                    Stmt::Assign { name, expr, to } => {
                        let result = ctx.eval(&expr).and_then(|q| match &to {
                            Some(u) => q.convert(u),
                            None => Ok(q),
                        });
                        let values = ctx.values;
                        if let Ok(q) = &result {
                            let written = match &expr {
                                Expr::Num { text, .. } if to.is_none() => Some(text.clone()),
                                Expr::Neg(x) if to.is_none() => match &**x {
                                    Expr::Num { text, .. } => Some(format!("-{text}")),
                                    _ => None,
                                },
                                _ => None,
                            };
                            vars.insert(name.clone(), Var { q: q.clone(), written });
                        } else {
                            vars.remove(&name);
                        }
                        Out::Assign { name, expr, values, result, mode, comment }
                    }
                    Stmt::Check { lhs, rel, rhs } => {
                        let sides = ctx.eval(&lhs).and_then(|l| {
                            let r = ctx.eval(&rhs)?;
                            l.cmp(&r, rel.symbol(false))?;
                            Ok((l, r))
                        });
                        Out::Check { lhs, rel, rhs, values: ctx.values, sides, mode, comment }
                    }
                    Stmt::Show { expr, to } => {
                        let result = ctx.eval(&expr).and_then(|q| match &to {
                            Some(u) => q.convert(u),
                            None => Ok(q),
                        });
                        Out::Show { expr, values: ctx.values, result, mode, comment }
                    }
                }
            }
        };
        out.push(o);
    }
    out
}

/// Evaluates the ```hardcalc blocks of a document.
pub fn evaluate_document(doc: &Document, wb: &mut Workbook) -> Vec<Out> {
    evaluate(&source_lines(doc), Some(wb))
}

/// The written lines of each ```hardcalc block, by block index – the sheet
/// evaluated as a whole, so later blocks see the variables of earlier ones.
pub fn render_blocks(doc: &Document, wb: &mut Workbook, r: Render) -> HashMap<usize, Vec<String>> {
    let mut outs = evaluate_document(doc, wb).into_iter();
    let mut map = HashMap::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        if let Block::Code { info, body } = b
            && is_sheet_info(info)
        {
            let n = body.iter().filter(|l| !matches!(expr::parse_line(l), Line::Mode(_))).count();
            let lines: Vec<String> = outs.by_ref().take(n).map(|o| r.line(&o)).collect();
            map.insert(i, lines);
        }
    }
    map
}

/// Messages of all lines that could not be calculated.
pub fn problems(out: &[Out]) -> Vec<String> {
    out.iter()
        .filter_map(|o| match o {
            Out::Error(e) => Some(e.clone()),
            Out::Assign { name, result: Err(e), .. } => Some(format!("{name}: {e}")),
            Out::Show { result: Err(e), .. } | Out::Check { sides: Err(e), .. } => Some(e.clone()),
            _ => None,
        })
        .collect()
}

struct Ctx<'a> {
    vars: &'a HashMap<String, Var>,
    wb: Option<&'a mut Workbook>,
    /// Shown numbers of the leaves used, for the substituted step.
    values: Values,
}

impl Ctx<'_> {
    fn eval(&mut self, e: &Expr) -> Result<Quantity, String> {
        match e {
            Expr::Num { value, unit, .. } => match unit {
                Some(u) => Quantity::with_unit(*value, u.clone()),
                None => Ok(Quantity::number(*value)),
            },
            Expr::Var(name) => {
                if let Some(v) = self.vars.get(name) {
                    let shown = v.written.clone().unwrap_or_else(|| format_sig(v.q.shown().0, 3, true));
                    self.values.insert(name.clone(), shown);
                    return Ok(v.q.clone());
                }
                if name == "π" {
                    return Ok(Quantity::number(std::f64::consts::PI));
                }
                // A name of the grid: a row name, a named cell …
                match self.cell(name) {
                    Ok(q) => Ok(q),
                    Err(_) => Err(format!("unknown variable {name}")),
                }
            }
            Expr::Cell(r) => self.cell(r),
            Expr::Neg(x) => Ok(self.eval(x)?.neg()),
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.eval(a)?, self.eval(b)?);
                match op {
                    Op::Add => a.add(&b),
                    Op::Sub => a.sub(&b),
                    Op::Mul => Ok(a.mul(&b)),
                    Op::Div => a.div(&b),
                    Op::Pow => a.pow(&b),
                }
            }
            Expr::Call(f, args) => {
                let args: Vec<Quantity> = args.iter().map(|a| self.eval(a)).collect::<Result<_, _>>()?;
                call(f, &args)
            }
        }
    }

    /// A value of the grid, by a reference in formula syntax.
    fn cell(&mut self, r: &str) -> Result<Quantity, String> {
        let Some(wb) = self.wb.as_deref_mut() else { return Err(format!("{r}: no tables")) };
        let q = match wb.eval_formula(r, 0) {
            Value::Num(n, unit) => Quantity::with_unit(n, unit)?,
            Value::Error(e) => return Err(format!("{r}: {e}")),
            Value::Null => return Err(format!("{r} is empty")),
            other => return Err(format!("{r} is not a number: {}", other.display(None, true))),
        };
        self.values.insert(r.to_string(), format_sig(q.shown().0, 3, true));
        Ok(q)
    }
}

fn call(f: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let one = || match args {
        [x] => Ok(x),
        _ => Err(format!("{f}() takes one argument")),
    };
    let plain = |x: &Quantity| {
        if x.dim.is_none() { Ok(x.value) } else { Err(format!("{f}() needs a number without unit")) }
    };
    let num = |v: f64| Ok(Quantity::number(v));
    match f {
        "sqrt" | "wurzel" => one()?.sqrt(),
        "abs" => {
            let x = one()?;
            Ok(Quantity { value: x.value.abs(), ..x.clone() })
        }
        "min" | "max" => {
            let (first, rest) = args.split_first().ok_or_else(|| format!("{f}() needs arguments"))?;
            rest.iter().try_fold(first.clone(), |acc, x| {
                let o = acc.cmp(x, f)?;
                let take_x = if f == "min" { o.is_gt() } else { o.is_lt() };
                Ok(if take_x { x.clone() } else { acc })
            })
        }
        "sin" => num(plain(one()?)?.sin()),
        "cos" => num(plain(one()?)?.cos()),
        "tan" => num(plain(one()?)?.tan()),
        "asin" | "acos" | "atan" => {
            let x = plain(one()?)?;
            let rad = match f {
                "asin" => x.asin(),
                "acos" => x.acos(),
                _ => x.atan(),
            };
            Quantity::with_unit(rad.to_degrees(), crate::units::Unit::symbol("°"))
        }
        "ln" => num(plain(one()?)?.ln()),
        "log" => num(plain(one()?)?.log10()),
        "exp" => num(plain(one()?)?.exp()),
        "round" => match args {
            [x, n] => {
                let digits = plain(n)? as i32;
                let (shown, unit) = x.shown();
                let r = crate::value::round_to(shown, digits);
                match unit {
                    Some(u) => Quantity::with_unit(r, u),
                    None => num(r),
                }
            }
            _ => Err("round() takes a value and the number of digits".into()),
        },
        _ => Err(format!("unknown function {f}()")),
    }
}

/// Options for writing a sheet as text.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Render {
    pub style: TextStyle,
}


impl Render {
    fn num(&self, s: &str) -> String {
        self.style.number(s)
    }

    fn q(&self, q: &Quantity) -> String {
        let s = q.format(!self.style.point);
        if self.style.ascii {
            let (v, u) = q.shown();
            let n = format_sig(v, 3, !self.style.point);
            match u {
                Some(u) => format!("{n} {u}"),
                None => n,
            }
        } else {
            s
        }
    }

    fn name(&self, n: &str) -> String {
        if self.style.ascii { expr::ascii_name(n) } else { n.to_string() }
    }

    fn formula(&self, e: &Expr) -> String {
        to_text(e, self.style, &|_| None)
    }

    fn substituted(&self, e: &Expr, values: &Values) -> String {
        to_text(e, self.style, &|leaf| match leaf {
            Expr::Var(n) | Expr::Cell(n) => values.get(n).map(|v| self.num(v)),
            // Units are left out in the substituted step.
            Expr::Num { text, .. } => Some(self.num(text)),
            _ => None,
        })
    }

    /// `formula = substituted = result`, steps as `mode` asks; steps that
    /// would repeat the one before are left out.
    fn steps(&self, e: &Expr, values: &Values, q: &Quantity, mode: Mode) -> Vec<String> {
        let result = self.q(q);
        // An input in its own unit is shown as written: 0,60 kN/m².
        let literal = match e {
            Expr::Num { unit, .. } => Some(unit),
            Expr::Neg(x) => match &**x {
                Expr::Num { unit, .. } => Some(unit),
                _ => None,
            },
            _ => None,
        };
        if let Some(unit) = literal {
            let same = match unit {
                Some(u) => q.display.as_ref() == Some(u),
                None => q.dim.is_none() && q.display.is_none(),
            };
            return vec![if same { self.formula(e) } else { result }];
        }
        let mut steps = Vec::new();
        if mode != Mode::Result {
            steps.push(self.formula(e));
            if mode == Mode::Full && !matches!(e, Expr::Var(_) | Expr::Cell(_)) {
                let s = self.substituted(e, values);
                if !steps.contains(&s) && s != result {
                    steps.push(s);
                }
            }
        }
        steps.push(result);
        steps
    }

    /// One line of text per evaluated line.
    pub fn lines(&self, out: &[Out]) -> Vec<String> {
        out.iter().map(|o| self.line(o)).collect()
    }

    pub fn line(&self, o: &Out) -> String {
        let with_comment = |s: String, c: &Option<String>| match c {
            Some(c) if !c.is_empty() => format!("{s}   ' {c}"),
            _ => s,
        };
        match o {
            Out::Blank => String::new(),
            Out::Heading { level, text } => format!("{} {text}", "#".repeat(*level)),
            Out::Comment(c) => format!("' {c}"),
            Out::Error(e) => format!("⚠ {e}"),
            Out::Assign { name, expr, values, result, mode, comment } => {
                let s = match result {
                    Ok(q) => format!("{} = {}", self.name(name), self.steps(expr, values, q, *mode).join(" = ")),
                    Err(e) => format!("{} = {}   ⚠ {e}", self.name(name), self.formula(expr)),
                };
                with_comment(s, comment)
            }
            Out::Show { expr, values, result, mode, comment } => {
                let s = match result {
                    Ok(q) => self.steps(expr, values, q, *mode).join(" = "),
                    Err(e) => format!("{}   ⚠ {e}", self.formula(expr)),
                };
                with_comment(s, comment)
            }
            Out::Check { lhs, rel, rhs, values, sides, mode, comment } => {
                let s = match sides {
                    Ok((l, r)) => {
                        let side = |e: &Expr, q: &Quantity| self.steps(e, values, q, (*mode).min_formula()).join(" = ");
                        let v = verdict(*rel, l, r);
                        let eta = match v.eta {
                            Some(eta) => format!("   {} = {}", if self.style.ascii { "eta" } else { "η" }, format_sig_fixed(eta, 2, !self.style.point)),
                            None => String::new(),
                        };
                        let mark = match (v.ok, self.style.ascii) {
                            (true, false) => "✓",
                            (false, false) => "✗",
                            (true, true) => "ok",
                            (false, true) => "NOT OK",
                        };
                        format!("{} {} {}{eta} {mark}", side(lhs, l), rel.symbol(self.style.ascii), side(rhs, r))
                    }
                    Err(e) => format!("{} {} {}   ⚠ {e}", self.formula(lhs), rel.symbol(self.style.ascii), self.formula(rhs)),
                };
                with_comment(s, comment)
            }
        }
    }
}

impl Mode {
    /// Sides of a check show at most formula and result.
    fn min_formula(self) -> Mode {
        if self == Mode::Full { Mode::Formula } else { self }
    }
}

/// `v` with `d` decimals.
fn format_sig_fixed(v: f64, d: usize, comma: bool) -> String {
    let s = format!("{:.*}", d, crate::value::round_to(v, d as i32));
    if comma { s.replace('.', ",") } else { s }
}

/// The unit of a quantity, written for people (`kN/m²`).
pub fn unit_text(q: &Quantity) -> Option<String> {
    q.unit().map(|u| pretty(&u))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Vec<String> {
        let lines: Vec<String> = src.lines().map(str::to_string).collect();
        Render::default().lines(&evaluate(&lines, None))
    }

    #[test]
    fn concept_example() {
        let out = run("## Bending check
f_yk   = 235 N/mm²          ' yield strength
γ_M    = 1.1
L      = 5.0 m
g_k    = 4.0 kN/m
q_k    = 3.0 kN/m
q_d    = 1.35·g_k + 1.5·q_k
M_Ed   = q_d·L^2 / 8
W_y    = 324 cm^3
σ_Ed   = M_Ed / W_y → N/mm²
f_yd   = f_yk / γ_M
σ_Ed ≤ f_yd");
        assert_eq!(
            out,
            [
                "## Bending check",
                "f_yk = 235 N/mm²   ' yield strength",
                "γ_M = 1.1",
                "L = 5.0 m",
                "g_k = 4.0 kN/m",
                "q_k = 3.0 kN/m",
                "q_d = 1.35·g_k + 1.5·q_k = 1.35·4.0 + 1.5·3.0 = 9.90 kN/m",
                "M_Ed = q_d·L^2/8 = 9.90·5.0^2/8 = 30.9 kN·m",
                "W_y = 324 cm³",
                "σ_Ed = M_Ed/W_y = 30.9/324 = 95.5 N/mm²",
                "f_yd = f_yk/γ_M = 235/1.1 = 214 N/mm²",
                "σ_Ed = 95.5 N/mm² ≤ f_yd = 214 N/mm²   η = 0.45 ✓",
            ]
        );
    }

    #[test]
    fn modes() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let out = run("a = 2 m\nb = 3 m\n#result\nA = a·b\n#formula\nA = a·b\nA = a·b #full");
        assert_eq!(out[2], "A = 6,00 m²");
        assert_eq!(out[3], "A = a·b = 6,00 m²");
        assert_eq!(out[4], "A = a·b = 2·3 = 6,00 m²");
    }

    #[test]
    fn errors_stay_on_their_line() {
        let out = run("a = 1 kN\nb = 2 m\nc = a + b\nd = c · 2\ne = (");
        assert_eq!(out[2], "c = a + b   ⚠ units do not match: kN + m");
        assert_eq!(out[3], "d = c·2   ⚠ unknown variable c");
        assert!(out[4].starts_with("⚠ e = (:"));
    }

    #[test]
    fn failed_check_and_conversion() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let out = run("N = 120 kN\nA = 10 cm²\nσ = N/A -> N/mm²\nf_yd = 23,5 kN/cm²\nσ >= f_yd");
        assert_eq!(out[2], "σ = N/A = 120/10 = 120 N/mm²");
        assert_eq!(out[4], "σ = 120 N/mm² ≥ f_yd = 23,5 kN/cm²   η = 1,96 ✗");
    }

    #[test]
    fn ascii_output() {
        let lines: Vec<String> = ["gamma_M = 1,5", "f = 30 kN/m² / gamma_M", "f <= 25 kN/m^2"].map(String::from).to_vec();
        let r = Render { style: TextStyle { ascii: true, point: true } };
        let out = r.lines(&evaluate(&lines, None));
        assert_eq!(out[1], "f = 30 kN/m^2/gamma_M = 30/1.5 = 20.0 kN/m^2");
        assert_eq!(out[2], "f = 20.0 kN/m^2 <= 25 kN/m^2   eta = 0.80 ok");
    }

    #[test]
    fn functions() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let out = run("a = 3 m\nb = 4 m\nc = sqrt(a^2 + b^2)\nm = max(a; b; 3500 mm)\nx = sin(30°)");
        assert_eq!(out[2], "c = sqrt(a^2 + b^2) = sqrt(3^2 + 4^2) = 5,00 m");
        assert_eq!(out[3], "m = max(a; b; 3500 mm) = max(3; 4; 3500) = 4,00 m");
        assert_eq!(out[4], "x = sin(30°) = sin(30) = 0,500");
    }
}
