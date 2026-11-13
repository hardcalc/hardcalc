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
pub mod library;
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
    val: Val,
    written: Option<String>,
}

/// The values of a range, one per point, and the range variable they run
/// over (`M` over `x` when `M = q·x·(L - x)/2` and `x = {0,,, 5} m`).
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub items: Vec<Quantity>,
    pub over: Option<String>,
}

/// A value while evaluating: one quantity or a range of them.
#[derive(Debug, Clone)]
enum Val {
    One(Quantity),
    Many(Series),
}

impl Series {
    /// The smallest and the largest value, the smallest in the unit the
    /// largest is shown in (`0 … 31.3 kN·m`).
    pub fn span(&self) -> Option<(f64, &Quantity)> {
        let lo = self.items.iter().min_by(|a, b| a.value.total_cmp(&b.value))?;
        let hi = self.items.iter().max_by(|a, b| a.value.total_cmp(&b.value))?;
        let start = match hi.shown().1 {
            Some(u) => lo.convert(&u).map_or_else(|_| lo.shown().0, |f| f.shown().0),
            None => lo.shown().0,
        };
        Some((start, hi))
    }
}

impl Val {
    fn len(&self) -> Option<usize> {
        match self {
            Val::One(_) => None,
            Val::Many(s) => Some(s.items.len()),
        }
    }

    fn at(&self, i: usize) -> Quantity {
        match self {
            Val::One(q) => q.clone(),
            Val::Many(s) => s.items[i].clone(),
        }
    }

    fn over(&self) -> Option<String> {
        match self {
            Val::Many(s) => s.over.clone(),
            Val::One(_) => None,
        }
    }

    fn one(self, what: &str) -> Result<Quantity, String> {
        match self {
            Val::One(q) => Ok(q),
            Val::Many(_) => Err(format!("{what} takes a single value, not a range")),
        }
    }

    fn map(self, f: impl Fn(&Quantity) -> Result<Quantity, String>) -> Result<Val, String> {
        match self {
            Val::One(q) => Ok(Val::One(f(&q)?)),
            Val::Many(s) => Ok(Val::Many(Series { items: s.items.iter().map(f).collect::<Result<_, _>>()?, over: s.over })),
        }
    }
}

/// The common length of the ranges among `vals` (`None`: all single values).
fn common_len(vals: &[&Val]) -> Result<Option<usize>, String> {
    let mut n = None;
    for v in vals {
        if let Some(k) = v.len() {
            match n {
                Some(m) if m != k => return Err(format!("ranges of different length: {m} and {k} values")),
                _ => n = Some(k),
            }
        }
    }
    Ok(n)
}

/// `f` applied point by point: a range wherever one of `vals` is a range.
fn broadcast(vals: &[&Val], mut f: impl FnMut(&[Quantity]) -> Result<Quantity, String>) -> Result<Val, String> {
    match common_len(vals)? {
        None => Ok(Val::One(f(&vals.iter().map(|v| v.at(0)).collect::<Vec<_>>())?)),
        Some(n) => {
            let over = vals.iter().find_map(|v| v.over());
            let items = (0..n).map(|i| f(&vals.iter().map(|v| v.at(i)).collect::<Vec<_>>())).collect::<Result<_, _>>()?;
            Ok(Val::Many(Series { items, over }))
        }
    }
}

/// An own function: its parameters and its formula.
#[derive(Debug, Clone, PartialEq)]
pub struct Func {
    pub params: Vec<String>,
    pub expr: Expr,
}

/// Functions of the formula library and of earlier lines, by name.
pub type Funcs = HashMap<String, Func>;

/// Where a check over a range is decisive: the value of the range variable
/// there, and how many points were checked.
#[derive(Debug, Clone, PartialEq)]
pub struct At {
    pub over: Option<String>,
    pub x: Option<Quantity>,
    pub n: usize,
}

/// A chart of ranges, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    /// The curves: what is drawn (as written) and its values.
    pub ys: Vec<(String, Series)>,
    /// The x values and their name; `None`: numbered points.
    pub x: Option<(String, Series)>,
    /// `line` (default), `area`, `bar` or `scatter`.
    pub kind: String,
    pub title: Option<String>,
}

/// One evaluated line.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Blank,
    Heading { level: usize, text: String },
    Comment(String),
    /// `series`: the values when the result is a range (`result` holds the first).
    Assign { name: String, expr: Expr, values: Values, result: Result<Quantity, String>, series: Option<Series>, mode: Mode, comment: Option<String>, label: Option<String> },
    /// `at`: for a check over a range, where it is decisive (`sides` hold the values there).
    Check { lhs: Expr, rel: Rel, rhs: Expr, values: Values, sides: Result<(Quantity, Quantity), String>, at: Option<At>, mode: Mode, comment: Option<String>, label: Option<String> },
    Show { expr: Expr, values: Values, result: Result<Quantity, String>, series: Option<Series>, mode: Mode, comment: Option<String>, label: Option<String> },
    Def { name: String, params: Vec<String>, expr: Expr, comment: Option<String>, label: Option<String> },
    /// `name`: the label, the name of the chart.
    Plot { name: String, plot: Result<Plot, String>, comment: Option<String>, label: Option<String> },
    Error(String),
}

impl Out {
    /// The label of a calculated line (`[Bending]`).
    pub fn label(&self) -> Option<&str> {
        match self {
            Out::Assign { label, .. } | Out::Check { label, .. } | Out::Show { label, .. } | Out::Def { label, .. } | Out::Plot { label, .. } => label.as_deref(),
            _ => None,
        }
    }

    /// The comment at the end of a calculated line, if not empty.
    pub fn comment(&self) -> Option<&str> {
        match self {
            Out::Assign { comment, .. } | Out::Check { comment, .. } | Out::Show { comment, .. } | Out::Def { comment, .. } | Out::Plot { comment, .. } => {
                comment.as_deref().filter(|c| !c.is_empty())
            }
            _ => None,
        }
    }
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

/// Name of a plot without a label.
pub fn plot_name(n: usize) -> String {
    format!("plot {}", n + 1)
}

pub fn evaluate(lines: &[String], wb: Option<&mut Workbook>) -> Vec<Out> {
    evaluate_with(lines, wb, &Funcs::new())
}

/// Evaluates the lines with the functions `library` already known.
pub fn evaluate_with(lines: &[String], mut wb: Option<&mut Workbook>, library: &Funcs) -> Vec<Out> {
    let mut vars: HashMap<String, Var> = HashMap::new();
    let mut funcs = library.clone();
    let mut mode = Mode::default();
    let mut plots = 0;
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
            Line::Stmt { stmt: Some(stmt), comment, mode: own, label } => {
                let mode = own.unwrap_or(mode);
                let mut ctx = Ctx { vars: &vars, funcs: &funcs, locals: Vec::new(), wb: wb.as_deref_mut(), values: Values::new(), depth: 0 };
                match stmt {
                    Stmt::Assign { name, expr, to } => {
                        let val = ctx.eval(&expr).and_then(|v| match &to {
                            Some(u) => v.map(|q| q.convert(u)),
                            None => Ok(v),
                        });
                        let values = ctx.values;
                        let (result, series) = match &val {
                            Ok(Val::One(q)) => (Ok(q.clone()), None),
                            Ok(Val::Many(s)) => match s.items.first() {
                                // A range of its own runs over this variable.
                                Some(q) => (Ok(q.clone()), Some(Series { items: s.items.clone(), over: s.over.clone().or_else(|| Some(name.clone())) })),
                                None => (Err("the range is empty".to_string()), None),
                            },
                            Err(e) => (Err(e.clone()), None),
                        };
                        match (&result, &series) {
                            (Ok(_), Some(s)) => {
                                vars.insert(name.clone(), Var { val: Val::Many(s.clone()), written: None });
                            }
                            (Ok(q), None) => {
                                let written = match &expr {
                                    Expr::Num { text, .. } if to.is_none() => Some(text.clone()),
                                    Expr::Neg(x) if to.is_none() => match &**x {
                                        Expr::Num { text, .. } => Some(format!("-{text}")),
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                vars.insert(name.clone(), Var { val: Val::One(q.clone()), written });
                            }
                            (Err(_), _) => {
                                vars.remove(&name);
                            }
                        }
                        Out::Assign { name, expr, values, result, series, mode, comment, label }
                    }
                    Stmt::Check { lhs, rel, rhs } => {
                        let checked = ctx.eval(&lhs).and_then(|l| Ok((l, ctx.eval(&rhs)?))).and_then(|(l, r)| check(&l, &r, rel, &vars));
                        let (sides, at) = match checked {
                            Ok((l, r, at)) => (Ok((l, r)), at),
                            Err(e) => (Err(e), None),
                        };
                        Out::Check { lhs, rel, rhs, values: ctx.values, sides, at, mode, comment, label }
                    }
                    Stmt::Show { expr, to } => {
                        let val = ctx.eval(&expr).and_then(|v| match &to {
                            Some(u) => v.map(|q| q.convert(u)),
                            None => Ok(v),
                        });
                        let (result, series) = match val {
                            Ok(Val::One(q)) => (Ok(q), None),
                            Ok(Val::Many(s)) => match s.items.first() {
                                Some(q) => (Ok(q.clone()), Some(s)),
                                None => (Err("the range is empty".to_string()), None),
                            },
                            Err(e) => (Err(e), None),
                        };
                        Out::Show { expr, values: ctx.values, result, series, mode, comment, label }
                    }
                    Stmt::Def { name, params, expr } => {
                        funcs.insert(name.clone(), Func { params: params.clone(), expr: expr.clone() });
                        Out::Def { name, params, expr, comment, label }
                    }
                    Stmt::Plot { ys, x, opts } => {
                        let name = label.clone().unwrap_or_else(|| plot_name(plots));
                        plots += 1;
                        let plot = ctx.plot(&ys, x.as_ref(), &opts, &vars);
                        Out::Plot { name, plot, comment, label }
                    }
                }
            }
        };
        out.push(o);
    }
    out
}

/// A check point by point: the decisive pair (the one with the highest
/// utilisation, among those that fail if any do) and, over a range, where
/// it is.
fn check(l: &Val, r: &Val, rel: Rel, vars: &HashMap<String, Var>) -> Result<(Quantity, Quantity, Option<At>), String> {
    let Some(n) = common_len(&[l, r])? else {
        let (l, r) = (l.at(0), r.at(0));
        l.cmp(&r, rel.symbol(false))?;
        return Ok((l, r, None));
    };
    if n == 0 {
        return Err("the range is empty".into());
    }
    let mut worst: Option<(usize, bool, f64)> = None;
    for i in 0..n {
        let (a, b) = (l.at(i), r.at(i));
        a.cmp(&b, rel.symbol(false))?;
        let v = verdict(rel, &a, &b);
        let eta = v.eta.unwrap_or(f64::INFINITY);
        let better = match worst {
            None => true,
            Some((_, ok, e)) => (ok && !v.ok) || (ok == v.ok && eta > e),
        };
        if better {
            worst = Some((i, v.ok, eta));
        }
    }
    let i = worst.map_or(0, |(i, ..)| i);
    let over = l.over().or_else(|| r.over());
    let x = over.as_ref().and_then(|o| match vars.get(o).map(|v| &v.val) {
        Some(Val::Many(s)) if s.items.len() == n => Some(s.items[i].clone()),
        _ => None,
    });
    Ok((l.at(i), r.at(i), Some(At { over, x, n })))
}

/// Evaluates the ```hardcalc blocks of a document, with the formula library.
pub fn evaluate_document(doc: &Document, wb: &mut Workbook) -> Vec<Out> {
    evaluate_with(&source_lines(doc), Some(wb), &library::functions())
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

/// Whether a source line is a `plot(…)`.
pub fn is_plot_line(src: &str) -> bool {
    matches!(expr::parse_line(src), Line::Stmt { stmt: Some(Stmt::Plot { .. }), .. })
}

/// The names of the plots of a document, in order, without evaluating it.
pub fn plot_names(doc: &Document) -> Vec<String> {
    let mut names = Vec::new();
    for line in source_lines(doc) {
        if let Line::Stmt { stmt: Some(Stmt::Plot { .. }), label, .. } = expr::parse_line(&line) {
            names.push(label.unwrap_or_else(|| plot_name(names.len())));
        }
    }
    names
}

/// Messages of all lines that could not be calculated.
pub fn problems(out: &[Out]) -> Vec<String> {
    out.iter()
        .filter_map(|o| match o {
            Out::Error(e) => Some(e.clone()),
            Out::Assign { name, result: Err(e), .. } => Some(format!("{name}: {e}")),
            Out::Show { result: Err(e), .. } | Out::Check { sides: Err(e), .. } => Some(e.clone()),
            Out::Plot { name, plot: Err(e), .. } => Some(format!("{name}: {e}")),
            _ => None,
        })
        .collect()
}

/// How deep own functions may call each other.
const MAX_DEPTH: usize = 64;

/// Points of the numerical integration (Simpson's rule; even).
const INTEGRATION_STEPS: usize = 256;

/// At most this many values in a range.
const MAX_RANGE: usize = 100_000;

struct Ctx<'a> {
    vars: &'a HashMap<String, Var>,
    funcs: &'a Funcs,
    /// Parameters of own functions and the variable of `int` / `diff`, innermost last.
    locals: Vec<(String, Quantity)>,
    wb: Option<&'a mut Workbook>,
    /// Shown numbers of the leaves used, for the substituted step.
    values: Values,
    depth: usize,
}

impl Ctx<'_> {
    fn eval(&mut self, e: &Expr) -> Result<Val, String> {
        match e {
            Expr::Num { value, unit, .. } => Ok(Val::One(match unit {
                Some(u) => Quantity::with_unit(*value, u.clone())?,
                None => Quantity::number(*value),
            })),
            Expr::Var(name) => {
                if let Some((_, q)) = self.locals.iter().rev().find(|(n, _)| n == name) {
                    return Ok(Val::One(q.clone()));
                }
                if let Some(v) = self.vars.get(name) {
                    if let Val::One(q) = &v.val {
                        let shown = v.written.clone().unwrap_or_else(|| format_sig(q.shown().0, 3, true));
                        self.values.insert(name.clone(), shown);
                    }
                    return Ok(v.val.clone());
                }
                if name == "π" {
                    return Ok(Val::One(Quantity::number(std::f64::consts::PI)));
                }
                // A name of the grid: a row name, a named cell …
                match self.cell(name) {
                    Ok(q) => Ok(Val::One(q)),
                    Err(_) => Err(format!("unknown variable {name}")),
                }
            }
            Expr::Cell(r) => Ok(Val::One(self.cell(r)?)),
            Expr::Neg(x) => self.eval(x)?.map(|q| Ok(q.neg())),
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.eval(a)?, self.eval(b)?);
                broadcast(&[&a, &b], |q| match op {
                    Op::Add => q[0].add(&q[1]),
                    Op::Sub => q[0].sub(&q[1]),
                    Op::Mul => Ok(q[0].mul(&q[1])),
                    Op::Div => q[0].div(&q[1]),
                    Op::Pow => q[0].pow(&q[1]),
                })
            }
            Expr::Range { items, unit } => self.range(items, unit.as_ref()),
            Expr::Call(f, args) if expr::is_integral(f, args) => self.integral(args),
            Expr::Call(f, args) if expr::is_derivative(f, args) => self.derivative(args),
            Expr::Call(f, args) => {
                let vals: Vec<Val> = args.iter().map(|a| self.eval(a)).collect::<Result<_, _>>()?;
                if let Some(func) = self.funcs.get(f).cloned() {
                    if func.params.len() != vals.len() {
                        return Err(format!("{f}() takes {} argument(s): {}", func.params.len(), func.params.join("; ")));
                    }
                    if self.depth >= MAX_DEPTH {
                        return Err(format!("{f}() calls itself without end"));
                    }
                    let refs: Vec<&Val> = vals.iter().collect();
                    return broadcast(&refs, |qs| {
                        let base = self.locals.len();
                        self.locals.extend(func.params.iter().cloned().zip(qs.iter().cloned()));
                        self.depth += 1;
                        let r = self.eval(&func.expr).and_then(|v| v.one(&format!("{f}()")));
                        self.depth -= 1;
                        self.locals.truncate(base);
                        r
                    });
                }
                let lower = f.to_lowercase();
                // min / max of one range: its smallest / largest value.
                if let ([Val::Many(s)], "min" | "max") = (vals.as_slice(), lower.as_str()) {
                    return Ok(Val::One(call(&lower, &s.items)?));
                }
                let refs: Vec<&Val> = vals.iter().collect();
                broadcast(&refs, |qs| call(&lower, qs))
            }
        }
    }

    /// `{1, 1.1,,, 2} m`: the values, an empty item filled with the step
    /// of the two values before it.
    fn range(&mut self, items: &[Option<Expr>], unit: Option<&crate::units::Unit>) -> Result<Val, String> {
        let mut out: Vec<Quantity> = Vec::new();
        let mut gap = false;
        for item in items {
            let Some(e) = item else {
                if out.len() < 2 {
                    return Err("a range needs two values before ,,, – they give the step: {1, 1.1,,, 2}".into());
                }
                gap = true;
                continue;
            };
            let q = self.eval(e)?.one("a range item")?;
            let q = match unit {
                Some(u) if q.dim.is_none() && q.display.is_none() => Quantity::with_unit(q.value, u.clone())?,
                Some(_) => return Err("a unit after } needs plain numbers in the range".into()),
                None => q,
            };
            if std::mem::take(&mut gap) {
                let (prev, last) = (&out[out.len() - 2], &out[out.len() - 1]);
                let step = last.sub(prev)?;
                if step.value == 0.0 {
                    return Err("the two values before ,,, are equal: no step".into());
                }
                let k = (q.sub(last)?.value / step.value).round();
                if k < 1.0 || (last.value + k * step.value - q.value).abs() > 1e-9 * q.value.abs().max(step.value.abs()) {
                    return Err(format!("the step of the range does not reach {}", q.format(false)));
                }
                if k as usize + out.len() > MAX_RANGE {
                    return Err(format!("a range has at most {MAX_RANGE} values"));
                }
                let last = last.clone();
                for i in 1..k as usize {
                    out.push(last.add(&step.mul(&Quantity::number(i as f64)))?);
                }
            }
            out.push(q);
        }
        if gap {
            return Err("a range ends with a value: {1, 1.1,,, 2}".into());
        }
        Ok(Val::Many(Series { items: out, over: None }))
    }

    /// `f` with the local variable `x` bound to `q`; a single value.
    fn with_local(&mut self, x: &str, q: Quantity, f: &Expr) -> Result<Quantity, String> {
        self.locals.push((x.to_string(), q));
        let r = self.eval(f).and_then(|v| v.one("the function under int() / diff()"));
        self.locals.pop();
        r
    }

    /// `int(f; x; a; b)` by Simpson's rule; bounds may be ranges.
    fn integral(&mut self, args: &[Expr]) -> Result<Val, String> {
        let Expr::Var(x) = &args[1] else { unreachable!() };
        let (a, b) = (self.eval(&args[2])?, self.eval(&args[3])?);
        broadcast(&[&a, &b], |q| {
            let (a, b) = (&q[0], &q[1]);
            let n = INTEGRATION_STEPS;
            let h = b.sub(a)?.div(&Quantity::number(n as f64))?;
            let mut sum: Option<Quantity> = None;
            for i in 0..=n {
                let t = a.add(&h.mul(&Quantity::number(i as f64)))?;
                let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
                let fx = self.with_local(x, t, &args[0])?.mul(&Quantity::number(w));
                sum = Some(match sum {
                    Some(s) => s.add(&fx)?,
                    None => fx,
                });
            }
            sum.unwrap_or_else(|| Quantity::number(0.0)).mul(&h).div(&Quantity::number(3.0))
        })
    }

    /// `diff(f; x; x0)` by the central difference; `x0` may be a range.
    fn derivative(&mut self, args: &[Expr]) -> Result<Val, String> {
        let Expr::Var(x) = &args[1] else { unreachable!() };
        let at = self.eval(&args[2])?;
        broadcast(&[&at], |q| {
            let x0 = &q[0];
            let mut h = x0.clone();
            h.value = if x0.value == 0.0 { 1e-6 } else { x0.value.abs() * 1e-6 };
            let up = self.with_local(x, x0.add(&h)?, &args[0])?;
            let down = self.with_local(x, x0.sub(&h)?, &args[0])?;
            up.sub(&down)?.div(&h.mul(&Quantity::number(2.0)))
        })
    }

    /// `plot(ys; x: …)`: the curves and their x values.
    fn plot(&mut self, ys: &[Expr], x: Option<&Expr>, opts: &[(String, String)], vars: &HashMap<String, Var>) -> Result<Plot, String> {
        let style = TextStyle::default();
        let mut curves = Vec::new();
        for y in ys {
            match self.eval(y)? {
                Val::Many(s) => curves.push((to_text(y, style, &|_| None), s)),
                Val::One(_) => return Err(format!("{} is a single value – plot draws ranges, e.g. x = {{0, 0.5,,, 5}} m", to_text(y, style, &|_| None))),
            }
        }
        let n = curves[0].1.items.len();
        if curves.iter().any(|(_, s)| s.items.len() != n) {
            return Err("the curves have different numbers of values".into());
        }
        // One axis for all curves: they must be of one kind.
        let dim = curves[0].1.items.first().map(|q| q.dim);
        if let Some((other, _)) = curves.iter().find(|(_, s)| s.items.first().map(|q| q.dim) != dim) {
            return Err(format!("{} and {other} are of different kinds – draw them in two charts", curves[0].0));
        }
        let x = match x {
            Some(e) => match self.eval(e)? {
                Val::Many(s) => Some((to_text(e, style, &|_| None), s)),
                Val::One(_) => return Err("x must be a range".into()),
            },
            None => curves[0].1.over.clone().and_then(|o| match vars.get(&o).map(|v| &v.val) {
                Some(Val::Many(s)) => Some((o, s.clone())),
                _ => None,
            }),
        };
        if let Some((name, s)) = &x
            && s.items.len() != n
        {
            return Err(format!("{name} has {} values, the curves {n}", s.items.len()));
        }
        let mut kind = "line".to_string();
        let mut title = None;
        for (k, v) in opts {
            match k.as_str() {
                "type" if matches!(v.as_str(), "line" | "area" | "bar" | "scatter") => kind = v.clone(),
                "type" => return Err(format!("type is line, area, bar or scatter, not '{v}'")),
                "title" => title = Some(v.clone()).filter(|t| !t.is_empty()),
                _ => {}
            }
        }
        Ok(Plot { ys: curves, x, kind, title })
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

/// The built-in functions of calculation sheets.
pub const FUNCTIONS: &[&str] = &[
    "sqrt", "abs", "min", "max", "round", "sin", "cos", "tan", "cot", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "ln", "log", "exp", "int",
    "diff",
];

fn call(f: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let one = || match args {
        [x] => Ok(x),
        _ => Err(format!("{f}() takes one argument")),
    };
    let plain = |x: &Quantity| {
        if x.dim.is_none() { Ok(x.value) } else { Err(format!("{f}() needs a number without unit")) }
    };
    let num = |v: f64| Ok(Quantity::number(v));
    let degrees = |rad: f64| Quantity::with_unit(rad.to_degrees(), crate::units::Unit::symbol("°"));
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
        "cot" => num(1.0 / plain(one()?)?.tan()),
        "sinh" => num(plain(one()?)?.sinh()),
        "cosh" => num(plain(one()?)?.cosh()),
        "tanh" => num(plain(one()?)?.tanh()),
        "asin" => degrees(plain(one()?)?.asin()),
        "acos" => degrees(plain(one()?)?.acos()),
        "atan" => degrees(plain(one()?)?.atan()),
        // The angle of the point (x, y): atan2(y; x), in the full circle.
        "atan2" => match args {
            [y, x] if y.dim == x.dim => degrees(y.value.atan2(x.value)),
            [_, _] => Err("atan2(y; x) needs y and x of the same kind".into()),
            _ => Err("atan2() takes y and x: atan2(y; x)".into()),
        },
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
        "int" => Err("int(f; x; a; b): the integral of f over x from a to b".into()),
        "diff" => Err("diff(f; x; x0): the derivative of f by x at x0".into()),
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

    /// A result: the value, or for a range its smallest and largest value
    /// and how many there are (`0 … 5.00 m (11)`).
    fn result(&self, q: &Quantity, series: Option<&Series>) -> String {
        match series.filter(|s| s.items.len() > 1).and_then(Series::span) {
            Some((lo, hi)) => format!("{} … {} ({})", format_sig(lo, 3, !self.style.point), self.q(hi), series.map_or(0, |s| s.items.len())),
            None => self.q(q),
        }
    }

    /// `formula = substituted = result`, steps as `mode` asks; steps that
    /// would repeat the one before are left out. A range shows no
    /// substituted step.
    fn steps(&self, e: &Expr, values: &Values, q: &Quantity, mode: Mode, series: Option<&Series>) -> Vec<String> {
        let result = self.result(q, series);
        let mode = if series.is_some() { mode.min_formula() } else { mode };
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

    /// The line as text: its label, the calculation, its comment.
    pub fn line(&self, o: &Out) -> String {
        let mut s = match o.label() {
            Some(l) => format!("[{l}] {}", self.body(o)),
            None => self.body(o),
        };
        if let Some(c) = o.comment() {
            s.push_str(&format!("   ' {c}"));
        }
        s
    }

    /// The calculation of a line, without its label and comment.
    pub fn body(&self, o: &Out) -> String {
        match o {
            Out::Blank => String::new(),
            Out::Heading { level, text } => format!("{} {text}", "#".repeat(*level)),
            Out::Comment(c) => format!("' {c}"),
            Out::Error(e) => format!("⚠ {e}"),
            Out::Assign { name, expr, values, result, series, mode, .. } => match result {
                Ok(q) => format!("{} = {}", self.name(name), self.steps(expr, values, q, *mode, series.as_ref()).join(" = ")),
                Err(e) => format!("{} = {}   ⚠ {e}", self.name(name), self.formula(expr)),
            },
            Out::Show { expr, values, result, series, mode, .. } => match result {
                Ok(q) => self.steps(expr, values, q, *mode, series.as_ref()).join(" = "),
                Err(e) => format!("{}   ⚠ {e}", self.formula(expr)),
            },
            Out::Check { lhs, rel, rhs, values, sides, at, mode, .. } => match sides {
                Ok((l, r)) => {
                    let side = |e: &Expr, q: &Quantity| self.steps(e, values, q, (*mode).min_formula(), None).join(" = ");
                    let v = verdict(*rel, l, r);
                    let eta_name = match (self.style.ascii, at.is_some()) {
                        (true, false) => "eta",
                        (false, false) => "η",
                        (true, true) => "max eta",
                        (false, true) => "max η",
                    };
                    let mut eta = match v.eta {
                        Some(eta) => format!("   {eta_name} = {}", format_sig_fixed(eta, 2, !self.style.point)),
                        None => String::new(),
                    };
                    // Over a range: where it is decisive.
                    if let Some(At { over, x, n }) = at {
                        match (over, x) {
                            (Some(o), Some(x)) => eta.push_str(&format!(" at {} = {}", self.name(o), self.q(x))),
                            _ => eta.push_str(&format!(" ({n} values)")),
                        }
                    }
                    let mark = match (v.ok, self.style.ascii) {
                        (true, false) => "✓",
                        (false, false) => "✗",
                        (true, true) => "ok",
                        (false, true) => "NOT OK",
                    };
                    format!("{} {} {}{eta} {mark}", side(lhs, l), rel.symbol(self.style.ascii), side(rhs, r))
                }
                Err(e) => format!("{} {} {}   ⚠ {e}", self.formula(lhs), rel.symbol(self.style.ascii), self.formula(rhs)),
            },
            Out::Def { name, params, expr, .. } => {
                let params: Vec<String> = params.iter().map(|p| self.name(p)).collect();
                format!("{}({}) = {}", self.name(name), params.join("; "), self.formula(expr))
            }
            Out::Plot { plot, .. } => match plot {
                Ok(p) => {
                    let ys: Vec<&str> = p.ys.iter().map(|(n, _)| n.as_str()).collect();
                    let over = p.x.as_ref().map(|(n, _)| format!(" over {n}")).unwrap_or_default();
                    let n = p.ys[0].1.items.len();
                    format!("{} {}{over} ({n} values, {})", if self.style.ascii { "chart" } else { "▥" }, ys.join(", "), p.kind)
                }
                Err(e) => format!("⚠ {e}"),
            },
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

/// A unit after a number in running text: ` kN·m`, nothing for none.
pub fn unit_name_text(u: &crate::units::Unit) -> String {
    if u.is_none() { String::new() } else { format!(" {}", pretty(u)) }
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
    fn ranges_and_decisive_checks() {
        let out = run("x = {1,1.1,,,2}\ny = {0, 0.5,,, 5} m\nq = 10 kN/m\nL = 5 m\nM = q·y·(L - y)/2\nM_Rd = 40 kNm\nM ≤ M_Rd\nM_max = max(M)\nz = {1, 2, 4}\nw = {1,,2}\nbad = x + z\nM ≤ 20 kNm");
        assert_eq!(out[0], "x = {1; 1.1; …; 2} = 1.00 … 2.00 (11)");
        assert_eq!(out[1], "y = {0; 0.5; …; 5} m = 0 … 5.00 m (11)");
        assert_eq!(out[4], "M = q·y·(L - y)/2 = 0 … 31.3 kN·m (11)");
        assert_eq!(out[6], "M = 31.3 kN·m ≤ M_Rd = 40.0 kNm   max η = 0.78 at y = 2.50 m ✓");
        assert_eq!(out[7], "M_max = max(M) = 31.3 kN·m");
        assert_eq!(out[8], "z = {1; 2; 4} = 1.00 … 4.00 (3)");
        assert!(out[9].contains("⚠ a range needs two values before ,,,"), "{}", out[9]);
        assert!(out[10].contains("⚠ ranges of different length: 11 and 3 values"), "{}", out[10]);
        // The point with the highest utilisation decides.
        assert_eq!(out[11], "M = 31.3 kN·m ≤ 20 kNm   max η = 1.56 at y = 2.50 m ✗");
    }

    #[test]
    fn own_functions_integrals_and_derivatives() {
        let out = run("q = 10 kN/m\nL = 5 m\nM_ss(q; L) = q·L^2/8\nM_1 = M_ss(q; L)\nA = int(q; t; 0 m; L)\nI = int(t^2; t; 0; 3)\nD = diff(t^3; t; 2)\ny = {0,,,5} m\nx = {0, 2.5,,, 5} m\nV = diff(q·t·(L - t)/2; t; x)\nf(a) = f(a)\nr = f(1)\nM_ss(1)");
        assert_eq!(out[2], "M_ss(q; L) = q·L^2/8");
        assert_eq!(out[3], "M_1 = M_ss(q; L) = M_ss(10; 5) = 31.3 kN·m");
        assert_eq!(out[4], "A = ∫[0 m; L] q dt = ∫[0; 5] 10 dt = 50.0 kN");
        assert_eq!(out[5], "I = ∫[0; 3] t^2 dt = 9.00");
        assert_eq!(out[6], "D = d/dt(t^3)|t=2 = 12.0");
        assert_eq!(out[9], "V = d/dt(q·t·(L - t)/2)|t=x = -25.0 … 25.0 kN (3)");
        assert!(out[11].contains("calls itself without end"), "{}", out[11]);
        assert!(out[12].contains("M_ss() takes 2 argument(s): q; L"), "{}", out[12]);
    }

    #[test]
    fn plots_draw_ranges() {
        let lines: Vec<String> = "x = {0, 1,,, 4} m\nM = 2 kN·x\n[Moments] plot(M; type: area; title: Moment line)\nplot(M; x: x)\nplot(L)"
            .lines()
            .map(String::from)
            .collect();
        let out = evaluate(&lines, None);
        let Out::Plot { name, plot: Ok(p), .. } = &out[2] else { panic!("{:?}", out[2]) };
        assert_eq!((name.as_str(), p.kind.as_str(), p.title.as_deref()), ("Moments", "area", Some("Moment line")));
        assert_eq!((p.ys[0].0.as_str(), p.ys[0].1.items.len()), ("M", 5));
        assert_eq!(p.x.as_ref().map(|(n, _)| n.as_str()), Some("x"), "M runs over x");
        let Out::Plot { name, .. } = &out[3] else { panic!() };
        assert_eq!(name, "plot 2");
        assert_eq!(Render::default().line(&out[2]), "[Moments] ▥ M over x (5 values, area)");
        assert!(problems(&out[4..]).iter().any(|p| p.contains("unknown variable L")));
        let mixed = evaluate(&["x = {0, 1,,, 4} m".to_string(), "plot(x; 2·x/1 m)".to_string()], None);
        assert!(problems(&mixed).iter().any(|p| p.contains("x and 2·x/1 m are of different kinds")), "{:?}", problems(&mixed));
    }

    #[test]
    fn angle_functions() {
        let out = run("a = atan2(1 m; 1 m)\nb = cot(45°)\nc = cosh(0)\nd = tanh(0)");
        assert_eq!(out[0], "a = atan2(1 m; 1 m) = atan2(1; 1) = 45.0 °");
        assert_eq!(out[1], "b = cot(45°) = cot(45) = 1.00");
        assert_eq!(out[2], "c = cosh(0) = 1.00");
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
