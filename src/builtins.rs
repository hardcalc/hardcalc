//! Built-in functions: the list (with Excel names, for the functions tab,
//! export and import) and their evaluation.

use crate::engine::{binary, compare, map1, unit_name};
use crate::formula::BinOp;
use crate::units::Unit;
use crate::value::{civil_from_days, parse_date, parse_number, round_to, Value};

/// A built-in function.
pub struct Builtin {
    pub name: &'static str,
    /// Excel name (`None`: no equivalent, not exported).
    pub excel: Option<&'static str>,
    pub signature: &'static str,
    pub desc: &'static str,
    /// Kind of each argument for the export: `v` a value (a column means the
    /// cell in the formula's row), `r` a range (a column means all of it).
    /// The last letter repeats.
    pub args: &'static str,
}

const fn b(name: &'static str, excel: &'static str, signature: &'static str, desc: &'static str, args: &'static str) -> Builtin {
    Builtin { name, excel: Some(excel), signature, desc, args }
}

pub const BUILTINS: &[Builtin] = &[
    // Aggregates
    b("sum", "SUM", "sum(values …)", "sum of all numbers (empty cells and text are skipped)", "r"),
    b("avg", "AVERAGE", "avg(values …)", "average; also average, mean", "r"),
    b("min", "MIN", "min(values …)", "smallest number", "r"),
    b("max", "MAX", "max(values …)", "largest number", "r"),
    b("count", "COUNT", "count(values …)", "number of numbers", "r"),
    b("counta", "COUNTA", "counta(values …)", "number of non-empty values", "r"),
    b("countblank", "COUNTBLANK", "countblank(values …)", "number of empty cells", "r"),
    b("product", "PRODUCT", "product(values …)", "product of all numbers", "r"),
    b("median", "MEDIAN", "median(values …)", "middle value", "r"),
    b("stdev", "STDEV", "stdev(values …)", "standard deviation of a sample", "r"),
    // Conditional
    b("if", "IF", "if(condition, then, else)", "choose a value (only the branch taken is computed)", "v"),
    b("iferror", "IFERROR", "iferror(value, fallback)", "the value, or the fallback if it is an error", "v"),
    b("and", "AND", "and(conditions …)", "true if all are true", "v"),
    b("or", "OR", "or(conditions …)", "true if one is true", "v"),
    b("not", "NOT", "not(condition)", "the opposite", "v"),
    b("sumif", "SUMIF", "sumif(range, criterion, [sum_range])", "sum where the criterion holds, e.g. \">100\"", "rvr"),
    b("countif", "COUNTIF", "countif(range, criterion)", "count where the criterion holds", "rv"),
    b("averageif", "AVERAGEIF", "averageif(range, criterion, [avg_range])", "average where the criterion holds", "rvr"),
    b("sumifs", "SUMIFS", "sumifs(sum_range, range, criterion, …)", "sum where all criteria hold", "rrv"),
    b("countifs", "COUNTIFS", "countifs(range, criterion, …)", "count where all criteria hold", "rv"),
    // Lookup
    b("xlookup", "XLOOKUP", "xlookup(key, lookup, result, [not_found])", "the result next to the key", "vrrv"),
    b("match", "MATCH", "match(key, values, [0])", "position of the key (1 = first)", "vrv"),
    b("index", "INDEX", "index(values, n)", "the n-th value", "rv"),
    // Maths
    b("abs", "ABS", "abs(x)", "absolute value (also element-wise)", "v"),
    b("sqrt", "SQRT", "sqrt(x)", "square root; units with even powers", "v"),
    b("round", "ROUND", "round(x, [digits])", "round half away from zero", "v"),
    b("roundup", "ROUNDUP", "roundup(x, [digits])", "round away from zero", "v"),
    b("rounddown", "ROUNDDOWN", "rounddown(x, [digits])", "round towards zero", "v"),
    b("int", "INT", "int(x)", "round down to an integer", "v"),
    b("mod", "MOD", "mod(x, divisor)", "remainder, with the sign of the divisor", "v"),
    b("power", "POWER", "power(x, exponent)", "x to the power of the exponent", "v"),
    b("sign", "SIGN", "sign(x)", "-1, 0 or 1", "v"),
    b("ceiling", "CEILING", "ceiling(x, [step])", "round up to a multiple of step", "v"),
    b("floor", "FLOOR", "floor(x, [step])", "round down to a multiple of step", "v"),
    b("exp", "EXP", "exp(x)", "e to the power of x", "v"),
    b("ln", "LN", "ln(x)", "natural logarithm", "v"),
    b("log", "LOG", "log(x, [base])", "logarithm (base 10)", "v"),
    b("pi", "PI", "pi()", "3.14159…", "v"),
    // Dates
    b("today", "TODAY", "today()", "today's date", "v"),
    b("date", "DATE", "date(year, month, day)", "a date", "v"),
    b("year", "YEAR", "year(date)", "year of a date", "v"),
    b("month", "MONTH", "month(date)", "month of a date (1–12)", "v"),
    b("day", "DAY", "day(date)", "day of the month", "v"),
    b("weekday", "WEEKDAY", "weekday(date)", "day of the week (1 = Sunday … 7 = Saturday)", "v"),
    // Text
    b("len", "LEN", "len(text)", "number of characters", "v"),
    b("left", "LEFT", "left(text, [n])", "the first n characters", "v"),
    b("right", "RIGHT", "right(text, [n])", "the last n characters", "v"),
    b("mid", "MID", "mid(text, start, n)", "n characters from position start (1 = first)", "v"),
    b("upper", "UPPER", "upper(text)", "upper case", "v"),
    b("lower", "LOWER", "lower(text)", "lower case", "v"),
    b("proper", "PROPER", "proper(text)", "First Letters Upper Case", "v"),
    b("trim", "TRIM", "trim(text)", "without leading, trailing and double spaces", "v"),
    b("concat", "CONCAT", "concat(values …)", "all values joined; also concatenate, or a & b", "r"),
    b("textjoin", "TEXTJOIN", "textjoin(separator, skip_empty, values …)", "values joined with a separator", "vvr"),
    b("substitute", "SUBSTITUTE", "substitute(text, old, new)", "replace all occurrences", "v"),
    b("find", "FIND", "find(part, text, [start])", "position of part (case matters)", "v"),
    b("search", "SEARCH", "search(part, text, [start])", "position of part (any case)", "v"),
    b("rept", "REPT", "rept(text, n)", "the text n times", "v"),
    b("exact", "EXACT", "exact(a, b)", "true if both texts are equal (case matters)", "v"),
    b("value", "VALUE", "value(text)", "the number in a text", "v"),
    b("text", "TEXT", "text(value, format)", "a number or date as text: \"0.00\", \"#,##0\", \"0%\", \"dd.mm.yyyy\"", "v"),
    Builtin { name: "spark", excel: None, signature: "spark(values …)", desc: "sparkline ▁▃▅█ as text", args: "r" },
];

/// Other names for built-in functions (German, Excel synonyms).
pub const ALIASES: &[(&str, &str)] = &[
    ("average", "avg"),
    ("mean", "avg"),
    ("mittelwert", "avg"),
    ("summe", "sum"),
    ("anzahl", "count"),
    ("wenn", "if"),
    ("concatenate", "concat"),
    ("verketten", "concat"),
    ("länge", "len"),
    ("heute", "today"),
];

/// The built-in function called `name` (aliases resolved).
pub fn find(name: &str) -> Option<&'static Builtin> {
    let name = ALIASES.iter().find(|(a, _)| *a == name).map_or(name, |(_, n)| n);
    BUILTINS.iter().find(|b| b.name == name)
}

/// The built-in with Excel name `excel` (upper case).
pub fn from_excel(excel: &str) -> Option<&'static Builtin> {
    match excel {
        "CONCATENATE" => find("concat"),
        _ => BUILTINS.iter().find(|b| b.excel == Some(excel)),
    }
}

impl Builtin {
    /// Whether argument `i` is a range (see [`Builtin::args`]).
    pub fn range_arg(&self, i: usize) -> bool {
        let kinds = self.args.as_bytes();
        kinds.get(i).or(kinds.last()) == Some(&b'r')
    }
}

/// Collects the numbers of all arguments (flattened), skipping empty and text
/// cells like other spreadsheets do. Fails on errors and mixed units.
fn numbers(args: &[Value]) -> Result<(Vec<f64>, Unit), Value> {
    let mut nums = Vec::new();
    let mut unit: Option<Unit> = None;
    for v in args.iter().flat_map(|a| a.flatten()) {
        match v {
            Value::Error(_) => return Err(v.clone()),
            Value::Num(n, u) => {
                match &unit {
                    Some(prev) if prev != u => {
                        return Err(Value::err(format!("unit mismatch: {} vs {}", unit_name(prev), unit_name(u))));
                    }
                    _ => unit = Some(u.clone()),
                }
                nums.push(*n);
            }
            _ => {}
        }
    }
    Ok((nums, unit.unwrap_or_default()))
}

fn text(v: &Value) -> String {
    v.display(None, false)
}

/// A whole number argument.
fn int_arg(args: &[Value], i: usize, default: Option<i64>) -> Result<i64, Value> {
    match args.get(i) {
        Some(Value::Num(n, _)) => Ok(n.trunc() as i64),
        Some(Value::Null) | None => default.ok_or_else(|| Value::err(format!("argument {} is missing", i + 1))),
        Some(e @ Value::Error(_)) => Err(e.clone()),
        Some(other) => Err(Value::err(format!("not a number: {}", text(other)))),
    }
}

fn num_arg(args: &[Value], i: usize, default: Option<f64>) -> Result<f64, Value> {
    match args.get(i) {
        Some(Value::Num(n, _)) => Ok(*n),
        Some(Value::Null) | None => default.ok_or_else(|| Value::err(format!("argument {} is missing", i + 1))),
        Some(e @ Value::Error(_)) => Err(e.clone()),
        Some(other) => Err(Value::err(format!("not a number: {}", text(other)))),
    }
}

/// `">100"`, `"<>open"`, `"=x"`, `"x"` or a number → a test for values.
fn criterion(c: &Value) -> impl Fn(&Value) -> bool + use<> {
    let (op, rest) = match c {
        Value::Text(s) => {
            let s = s.trim();
            let (op, rest) = [(">=", BinOp::Ge), ("<=", BinOp::Le), ("<>", BinOp::Ne), (">", BinOp::Gt), ("<", BinOp::Lt), ("=", BinOp::Eq)]
                .into_iter()
                .find_map(|(p, op)| s.strip_prefix(p).map(|r| (op, r.trim().to_string())))
                .unwrap_or((BinOp::Eq, s.to_string()));
            (op, Some(rest))
        }
        _ => (BinOp::Eq, None),
    };
    let want = match (&rest, c) {
        (Some(r), _) => match parse_number(r) {
            Some((n, _)) => Value::num(n),
            None => match parse_date(r) {
                Some(d) => Value::Date(d),
                None if r.is_empty() => Value::Null,
                None => Value::Text(r.clone()),
            },
        },
        (None, v) => v.clone(),
    };
    move |v: &Value| {
        // Units do not stop a comparison with a plain number.
        let v = match (v, &want) {
            (Value::Num(n, _), Value::Num(_, u)) => Value::Num(*n, u.clone()),
            (Value::Text(a), Value::Text(b)) if matches!(op, BinOp::Eq | BinOp::Ne) => {
                return (a.to_lowercase() == b.to_lowercase()) == (op == BinOp::Eq);
            }
            _ => v.clone(),
        };
        matches!(compare(op, v, want.clone()), Value::Bool(true))
    }
}

/// Items of the range arguments, matched by position.
fn items(v: &Value) -> Vec<Value> {
    v.flatten().into_iter().cloned().collect()
}

fn today() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.as_secs() / 86_400) as i64).unwrap_or(0)
}

/// Days since 1970-01-01 of a calendar date (month may overflow, like Excel).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let (y, m) = (y + (m - 1).div_euclid(12), (m - 1).rem_euclid(12) + 1);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `text(value, format)`: numbers with `0`, `0.00`, `#,##0.00`, `0%`; dates with `dd`, `mm`, `yyyy`.
fn format_text(v: &Value, fmt: &str) -> Value {
    match v {
        Value::Date(d) => {
            let (y, m, day) = civil_from_days(*d);
            let s = fmt.replace("yyyy", &format!("{y:04}")).replace("YYYY", &format!("{y:04}"));
            let s = s.replace("yy", &format!("{:02}", y % 100)).replace("mm", &format!("{m:02}")).replace("MM", &format!("{m:02}"));
            Value::Text(s.replace("dd", &format!("{day:02}")).replace("DD", &format!("{day:02}")))
        }
        Value::Num(n, _) => {
            let percent = fmt.ends_with('%');
            let core = fmt.trim_end_matches('%');
            let n = if percent { n * 100.0 } else { *n };
            let decimals = core.split_once('.').map_or(0, |(_, d)| d.chars().filter(|c| *c == '0' || *c == '#').count());
            let mut s = format!("{:.*}", decimals, round_to(n, decimals as i32));
            if core.contains(',') {
                let (int, frac) = s.split_once('.').map_or((s.clone(), None), |(i, f)| (i.to_string(), Some(f.to_string())));
                let (sign, digits) = int.strip_prefix('-').map_or(("", int.as_str()), |d| ("-", d));
                let grouped: Vec<String> = digits.as_bytes().rchunks(3).rev().map(|c| String::from_utf8_lossy(c).into_owned()).collect();
                s = format!("{sign}{}{}", grouped.join(","), frac.map(|f| format!(".{f}")).unwrap_or_default());
            }
            Value::Text(if percent { format!("{s}%") } else { s })
        }
        other => Value::Text(text(other)),
    }
}

/// Applies `f` to the first argument (element-wise for vectors).
fn each(args: &[Value], f: &dyn Fn(Value) -> Value) -> Value {
    match args.first() {
        Some(x) => map1(x.clone(), &|v| match v {
            e @ Value::Error(_) => e,
            v => f(v),
        }),
        None => Value::err("an argument is missing"),
    }
}

fn number_fn(args: &[Value], f: &dyn Fn(f64, Unit) -> Value) -> Value {
    each(args, &|v| match v {
        Value::Num(n, u) => f(n, u),
        Value::Null => Value::Null,
        other => Value::err(format!("not a number: {}", text(&other))),
    })
}

/// Evaluates built-in function `name` (aliases already resolved by [`find`]).
pub fn call(name: &str, args: Vec<Value>) -> Value {
    let Some(builtin) = find(name) else { return Value::err(format!("unknown function '{name}'")) };
    let name = builtin.name;
    let agg = |f: &dyn Fn(&[f64]) -> Option<f64>| match numbers(&args) {
        Ok((nums, unit)) => match f(&nums) {
            Some(n) => Value::Num(n, unit),
            None => Value::err(format!("{name}: no numbers")),
        },
        Err(e) => e,
    };
    let try_ = |r: Result<Value, Value>| r.unwrap_or_else(|e| e);
    match name {
        "sum" => agg(&|n| Some(n.iter().sum())),
        "avg" => agg(&|n| (!n.is_empty()).then(|| n.iter().sum::<f64>() / n.len() as f64)),
        "min" => agg(&|n| n.iter().copied().reduce(f64::min)),
        "max" => agg(&|n| n.iter().copied().reduce(f64::max)),
        "median" => agg(&|n| {
            let mut v = n.to_vec();
            v.sort_by(|a, b| a.total_cmp(b));
            let m = v.len() / 2;
            match v.len() {
                0 => None,
                l if l % 2 == 1 => Some(v[m]),
                _ => Some((v[m - 1] + v[m]) / 2.0),
            }
        }),
        "stdev" => agg(&|n| {
            (n.len() > 1).then(|| {
                let mean = n.iter().sum::<f64>() / n.len() as f64;
                (n.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n.len() - 1) as f64).sqrt()
            })
        }),
        "count" => match numbers(&args) {
            Ok((nums, _)) => Value::num(nums.len() as f64),
            Err(e) => e,
        },
        "counta" => Value::num(args.iter().flat_map(|a| a.flatten()).filter(|v| !matches!(v, Value::Null)).count() as f64),
        "countblank" => Value::num(args.iter().flat_map(|a| a.flatten()).filter(|v| matches!(v, Value::Null)).count() as f64),
        "product" => args
            .iter()
            .flat_map(|a| a.flatten())
            .filter(|v| !matches!(v, Value::Null | Value::Text(_)))
            .fold(Value::num(1.0), |acc, v| binary(BinOp::Mul, acc, v.clone())),
        "spark" => match numbers(&args) {
            Ok((nums, _)) => Value::Text(crate::chart::sparkline(&nums)),
            Err(e) => e,
        },

        "if" => {
            let [c, a, b] = <[Value; 3]>::try_from(args).unwrap_or_else(|_| [Value::err("if needs 3 arguments"), Value::Null, Value::Null]);
            match c.truthy() {
                Ok(true) => a,
                Ok(false) => b,
                Err(e) => e,
            }
        }
        "iferror" => match args.as_slice() {
            [Value::Error(_), fallback] => fallback.clone(),
            [v, _] => v.clone(),
            _ => Value::err("iferror needs 2 arguments"),
        },
        "and" | "or" => {
            let mut result = name == "and";
            for v in args.iter().flat_map(|a| a.flatten()) {
                match v.truthy() {
                    Ok(t) if name == "and" => result &= t,
                    Ok(t) => result |= t,
                    Err(e) => return e,
                }
            }
            Value::Bool(result)
        }
        "not" => each(&args, &|v| match v.truthy() {
            Ok(t) => Value::Bool(!t),
            Err(e) => e,
        }),
        "sumif" | "averageif" | "countif" => {
            let (Some(range), Some(c)) = (args.first(), args.get(1)) else { return Value::err(format!("{name} needs a range and a criterion")) };
            let test = criterion(c);
            let range = items(range);
            let values = args.get(2).map(items).unwrap_or_else(|| range.clone());
            let picked: Vec<Value> = range.iter().zip(values).filter(|(r, _)| test(r)).map(|(_, v)| v).collect();
            match name {
                "countif" => Value::num(picked.len() as f64),
                "sumif" => call("sum", vec![Value::Vector(picked)]),
                _ => call("avg", vec![Value::Vector(picked)]),
            }
        }
        "sumifs" | "countifs" => {
            let (values, pairs) = if name == "sumifs" {
                let Some((first, rest)) = args.split_first() else { return Value::err("sumifs needs a sum range") };
                (Some(items(first)), rest)
            } else {
                (None, &args[..])
            };
            if pairs.is_empty() || pairs.len() % 2 != 0 {
                return Value::err(format!("{name} needs pairs of range and criterion"));
            }
            let tests: Vec<(Vec<Value>, _)> = pairs.chunks(2).map(|p| (items(&p[0]), criterion(&p[1]))).collect();
            let n = tests[0].0.len();
            let rows: Vec<usize> = (0..n).filter(|&i| tests.iter().all(|(r, t)| r.get(i).is_some_and(t))).collect();
            match values {
                Some(v) => call("sum", vec![Value::Vector(rows.iter().filter_map(|&i| v.get(i).cloned()).collect())]),
                None => Value::num(rows.len() as f64),
            }
        }

        "xlookup" => {
            let (Some(key), Some(look), Some(result)) = (args.first(), args.get(1), args.get(2)) else {
                return Value::err("xlookup needs key, lookup and result");
            };
            let test = criterion(&match key {
                Value::Text(t) => Value::Text(format!("={t}")),
                k => k.clone(),
            });
            match items(look).iter().position(&test) {
                Some(i) => items(result).get(i).cloned().unwrap_or(Value::err("result range is shorter")),
                None => args.get(3).cloned().unwrap_or_else(|| Value::err(format!("{} not found", text(key)))),
            }
        }
        "match" => {
            let (Some(key), Some(look)) = (args.first(), args.get(1)) else { return Value::err("match needs key and values") };
            let test = criterion(&match key {
                Value::Text(t) => Value::Text(format!("={t}")),
                k => k.clone(),
            });
            match items(look).iter().position(&test) {
                Some(i) => Value::num((i + 1) as f64),
                None => Value::err(format!("{} not found", text(key))),
            }
        }
        "index" => try_((|| {
            let values = items(args.first().ok_or_else(|| Value::err("index needs values"))?);
            let n = int_arg(&args, 1, None)?;
            values.get((n - 1).max(0) as usize).cloned().filter(|_| n >= 1).ok_or_else(|| Value::err(format!("index {n} is out of range")))
        })()),

        "abs" => number_fn(&args, &|n, u| Value::Num(n.abs(), u)),
        "sqrt" => number_fn(&args, &|n, u| match (n < 0.0, u.sqrt()) {
            (true, _) => Value::err("sqrt of negative number"),
            (false, Some(r)) => Value::Num(n.sqrt(), r),
            (false, None) => Value::err("sqrt of a unit with odd exponent"),
        }),
        "round" | "roundup" | "rounddown" => {
            let digits = match int_arg(&args, 1, Some(0)) {
                Ok(d) => d as i32,
                Err(e) => return e,
            };
            number_fn(&args, &|n, u| {
                let f = 10f64.powi(digits);
                let r = match name {
                    "round" => round_to(n, digits),
                    // A small tolerance against float artifacts (0.1 + 0.2).
                    "roundup" => (n.abs() * f - 1e-9).ceil().copysign(n) / f,
                    _ => (n.abs() * f + 1e-9).floor().copysign(n) / f,
                };
                Value::Num(r, u)
            })
        }
        "int" => number_fn(&args, &|n, u| Value::Num(n.floor(), u)),
        "sign" => number_fn(&args, &|n, _| Value::num(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 })),
        "exp" => number_fn(&args, &|n, _| Value::num(n.exp())),
        "ln" => number_fn(&args, &|n, _| if n > 0.0 { Value::num(n.ln()) } else { Value::err("ln of a number ≤ 0") }),
        "log" => {
            let base = match num_arg(&args, 1, Some(10.0)) {
                Ok(b) => b,
                Err(e) => return e,
            };
            number_fn(&args, &|n, _| if n > 0.0 { Value::num(n.log(base)) } else { Value::err("log of a number ≤ 0") })
        }
        "mod" => match num_arg(&args, 1, None) {
            Ok(0.0) => Value::err("mod by zero"),
            Ok(d) => number_fn(&args, &|n, u| Value::Num(n - d * (n / d).floor(), u)),
            Err(e) => e,
        },
        "power" => match args.get(1) {
            Some(e) => {
                let e = e.clone();
                each(&args, &|v| binary(BinOp::Pow, v, e.clone()))
            }
            None => Value::err("power needs an exponent"),
        },
        "ceiling" | "floor" => match num_arg(&args, 1, Some(1.0)) {
            Ok(0.0) => Value::num(0.0),
            Ok(step) => number_fn(&args, &|n, u| {
                let q = n / step;
                let q = if name == "ceiling" { (q - 1e-9).ceil() } else { (q + 1e-9).floor() };
                Value::Num(q * step, u)
            }),
            Err(e) => e,
        },
        "pi" => Value::num(std::f64::consts::PI),

        "today" => Value::Date(today()),
        "date" => try_((|| Ok(Value::Date(days_from_civil(int_arg(&args, 0, None)?, int_arg(&args, 1, None)?, int_arg(&args, 2, None)?))))()),
        "year" | "month" | "day" | "weekday" => each(&args, &|v| match v {
            Value::Date(d) => {
                let (y, m, day) = civil_from_days(d);
                Value::num(match name {
                    "year" => y as f64,
                    "month" => m as f64,
                    "day" => day as f64,
                    // 1970-01-01 was a Thursday (5 with Sunday = 1).
                    _ => ((d + 4).rem_euclid(7) + 1) as f64,
                })
            }
            Value::Null => Value::Null,
            other => Value::err(format!("not a date: {}", text(&other))),
        }),

        "len" => each(&args, &|v| Value::num(text(&v).chars().count() as f64)),
        "left" | "right" => match int_arg(&args, 1, Some(1)) {
            Ok(n) => each(&args, &|v| {
                let chars: Vec<char> = text(&v).chars().collect();
                let n = (n.max(0) as usize).min(chars.len());
                let part = if name == "left" { &chars[..n] } else { &chars[chars.len() - n..] };
                Value::Text(part.iter().collect())
            }),
            Err(e) => e,
        },
        "mid" => match (int_arg(&args, 1, None), int_arg(&args, 2, None)) {
            (Ok(start), Ok(n)) => each(&args, &|v| Value::Text(text(&v).chars().skip((start - 1).max(0) as usize).take(n.max(0) as usize).collect())),
            (Err(e), _) | (_, Err(e)) => e,
        },
        "upper" => each(&args, &|v| Value::Text(text(&v).to_uppercase())),
        "lower" => each(&args, &|v| Value::Text(text(&v).to_lowercase())),
        "proper" => each(&args, &|v| {
            let mut out = String::new();
            let mut start = true;
            for c in text(&v).chars() {
                if start { out.extend(c.to_uppercase()) } else { out.extend(c.to_lowercase()) }
                start = !c.is_alphanumeric();
            }
            Value::Text(out)
        }),
        "trim" => each(&args, &|v| Value::Text(text(&v).split_whitespace().collect::<Vec<_>>().join(" "))),
        "concat" => {
            if let Some(e) = args.iter().flat_map(|a| a.flatten()).find(|v| v.is_error()) {
                return e.clone();
            }
            Value::Text(args.iter().flat_map(|a| a.flatten()).map(text).collect())
        }
        "textjoin" => {
            let (Some(sep), Some(skip)) = (args.first(), args.get(1)) else { return Value::err("textjoin needs separator, skip_empty and values") };
            let skip = skip.truthy().unwrap_or(true);
            let parts: Vec<String> = args[2..].iter().flat_map(|a| a.flatten()).map(text).filter(|s| !(skip && s.is_empty())).collect();
            Value::Text(parts.join(&text(sep)))
        }
        "substitute" => match (args.get(1), args.get(2)) {
            (Some(old), Some(new)) => {
                let (old, new) = (text(old), text(new));
                each(&args, &|v| Value::Text(if old.is_empty() { text(&v) } else { text(&v).replace(&old, &new) }))
            }
            _ => Value::err("substitute needs text, old and new"),
        },
        "find" | "search" => {
            let (Some(part), Some(hay)) = (args.first(), args.get(1)) else { return Value::err(format!("{name} needs part and text")) };
            let start = match int_arg(&args, 2, Some(1)) {
                Ok(s) => (s - 1).max(0) as usize,
                Err(e) => return e,
            };
            let (part, hay) = if name == "search" { (text(part).to_lowercase(), text(hay).to_lowercase()) } else { (text(part), text(hay)) };
            let hay_chars: Vec<char> = hay.chars().collect();
            let rest: String = hay_chars.iter().skip(start).collect();
            match rest.find(&part) {
                Some(byte) => Value::num((start + rest[..byte].chars().count() + 1) as f64),
                None => Value::err(format!("'{part}' not found")),
            }
        }
        "rept" => match int_arg(&args, 1, None) {
            Ok(n) => each(&args, &|v| Value::Text(text(&v).repeat(n.max(0) as usize))),
            Err(e) => e,
        },
        "exact" => match args.as_slice() {
            [a, b] => Value::Bool(text(a) == text(b)),
            _ => Value::err("exact needs 2 texts"),
        },
        "value" => each(&args, &|v| match v {
            Value::Num(..) => v,
            other => match parse_number(&text(&other)) {
                Some((n, _)) => Value::num(n),
                None => Value::err(format!("not a number: {}", text(&other))),
            },
        }),
        "text" => match args.get(1) {
            Some(f) => {
                let f = text(f);
                each(&args, &|v| format_text(&v, &f))
            }
            None => Value::err("text needs a format"),
        },
        _ => Value::err(format!("unknown function '{name}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::num(x)
    }
    fn t(s: &str) -> Value {
        Value::Text(s.into())
    }
    fn v(items: Vec<Value>) -> Value {
        Value::Vector(items)
    }

    #[test]
    fn excel_like_functions() {
        assert_eq!(call("counta", vec![v(vec![n(1.0), Value::Null, t("x")])]), n(2.0));
        assert_eq!(call("median", vec![v(vec![n(3.0), n(1.0), n(2.0), n(10.0)])]), n(2.5));
        assert_eq!(call("roundup", vec![n(1.21), n(1.0)]), n(1.3));
        assert_eq!(call("rounddown", vec![n(-1.29), n(1.0)]), n(-1.2));
        assert_eq!(call("mod", vec![n(-7.0), n(3.0)]), n(2.0));
        assert_eq!(call("ceiling", vec![n(7.1), n(0.5)]), n(7.5));
        let amounts = v(vec![n(100.0), n(50.0), n(200.0)]);
        let status = v(vec![t("open"), t("paid"), t("open")]);
        assert_eq!(call("sumif", vec![amounts.clone(), t(">60")]), n(300.0));
        assert_eq!(call("sumif", vec![status.clone(), t("open"), amounts.clone()]), n(300.0));
        assert_eq!(call("countifs", vec![status.clone(), t("open"), amounts.clone(), t(">150")]), n(1.0));
        assert_eq!(call("sumifs", vec![amounts.clone(), status.clone(), t("<>open")]), n(50.0));
        assert_eq!(call("xlookup", vec![t("paid"), status.clone(), amounts.clone()]), n(50.0));
        assert_eq!(call("match", vec![n(200.0), amounts.clone()]), n(3.0));
        assert_eq!(call("index", vec![amounts, n(2.0)]), n(50.0));
        assert!(call("xlookup", vec![t("weg"), status.clone(), status]).is_error());
        assert_eq!(call("iferror", vec![Value::err("x"), n(0.0)]), n(0.0));
        assert_eq!(call("and", vec![Value::Bool(true), n(1.0)]), Value::Bool(true));
        assert_eq!(call("year", vec![Value::Date(days_from_civil(2026, 10, 11))]), n(2026.0));
        assert_eq!(call("date", vec![n(2026.0), n(13.0), n(1.0)]), Value::Date(days_from_civil(2027, 1, 1)));
        assert_eq!(call("weekday", vec![Value::Date(days_from_civil(2026, 10, 11))]), n(1.0), "a Sunday");
    }

    #[test]
    fn text_functions() {
        assert_eq!(call("len", vec![t("naïve")]), n(5.0));
        assert_eq!(call("left", vec![t("Rent"), n(3.0)]), t("Ren"));
        assert_eq!(call("right", vec![t("Rent")]), t("t"));
        assert_eq!(call("mid", vec![t("Internet"), n(3.0), n(3.0)]), t("ter"));
        assert_eq!(call("proper", vec![t("hello WORLD-wide")]), t("Hello World-Wide"));
        assert_eq!(call("trim", vec![t("  a   b ")]), t("a b"));
        assert_eq!(call("concatenate", vec![t("a"), n(1.0), v(vec![t("b"), t("c")])]), t("a1bc"));
        assert_eq!(call("textjoin", vec![t(", "), Value::Bool(true), v(vec![t("a"), Value::Null, t("b")])]), t("a, b"));
        assert_eq!(call("substitute", vec![t("a-b-c"), t("-"), t("+")]), t("a+b+c"));
        assert_eq!(call("find", vec![t("n"), t("Rent"), n(3.0)]), n(3.0));
        assert_eq!(call("search", vec![t("R"), t("rent")]), n(1.0));
        assert_eq!(call("upper", vec![v(vec![t("a"), t("b")])]), v(vec![t("A"), t("B")]), "element-wise");
        assert_eq!(call("value", vec![t("1.234,5")]), n(1234.5));
        assert_eq!(call("text", vec![n(1234.5), t("#,##0.00")]), t("1,234.50"));
        assert_eq!(call("text", vec![n(0.125), t("0.0%")]), t("12.5%"));
        assert_eq!(call("text", vec![Value::Date(days_from_civil(2026, 3, 4)), t("dd.mm.yyyy")]), t("04.03.2026"));
    }
}
