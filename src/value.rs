//! Runtime values and the scalar operations of the formula language.

use crate::units::Unit;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64, Unit),
    Text(String),
    /// Days since 1970-01-01.
    Date(i64),
    Vector(Vec<Value>),
    Error(String),
}

impl Value {
    pub fn num(n: f64) -> Value {
        Value::Num(n, Unit::none())
    }

    pub fn err(msg: impl Into<String>) -> Value {
        Value::Error(msg.into())
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// Flattens nested vectors into a list of scalars.
    pub fn flatten(&self) -> Vec<&Value> {
        match self {
            Value::Vector(items) => items.iter().flat_map(|v| v.flatten()).collect(),
            v => vec![v],
        }
    }

    pub fn truthy(&self) -> Result<bool, Value> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Num(n, _) => Ok(*n != 0.0),
            Value::Null => Ok(false),
            Value::Error(_) => Err(self.clone()),
            other => Err(Value::err(format!("not a boolean: {}", other.display(None, false)))),
        }
    }

    /// Human-readable rendering. `decimals` fixes the number of decimal places,
    /// `comma` selects `,` as decimal separator.
    pub fn display(&self, decimals: Option<u8>, comma: bool) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n, unit) => {
                let mut s = format_number(*n, decimals);
                if comma {
                    s = s.replace('.', ",");
                }
                if unit.is_none() { s } else { format!("{s} {unit}") }
            }
            Value::Text(s) => s.clone(),
            Value::Date(d) => {
                let (y, m, day) = civil_from_days(*d);
                format!("{y:04}-{m:02}-{day:02}")
            }
            Value::Vector(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.display(decimals, comma)).collect();
                format!("[{}]", parts.join("; "))
            }
            Value::Error(e) => format!("#ERR: {e}"),
        }
    }
}

/// Rounds half away from zero to `digits` places, tolerating binary float
/// artifacts (85.50 * 1.19 = 101.74499… rounds to 101.75).
pub fn round_to(n: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    let x = n * f;
    let nudge = x.signum() * x.abs().max(1.0) * 1e-12;
    (x + nudge).round() / f
}

fn format_number(n: f64, decimals: Option<u8>) -> String {
    match decimals {
        Some(d) => format!("{:.*}", d as usize, n),
        None if n.fract() == 0.0 && n.abs() < 1e15 => format!("{n:.0}"),
        None => {
            let s = format!("{n:.10}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }
}

/// Parses a number written with `.` or `,` as decimal separator and the other
/// one as thousands separator. Returns the number and whether `,` was the
/// decimal separator.
pub fn parse_number(s: &str) -> Option<(f64, bool)> {
    let (normalized, comma_decimal) = point_form(s)?;
    normalized.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| (n, comma_decimal))
}

/// A number as written in files: decimal point, no thousands separators
/// (`1.200,50` → `1200.50`, `1,5` → `1.5`; digits after the separator stay).
/// `None` if `s` is no number.
pub fn point_number(s: &str) -> Option<String> {
    let (normalized, _) = point_form(s)?;
    normalized.parse::<f64>().ok().filter(|n| n.is_finite())?;
    Some(normalized)
}

/// `s` without thousands separators and with `.` as decimal separator, and
/// whether `,` was the decimal separator.
///
/// Separators between thousands must really group the digits in threes
/// (`1.200.000`, `1 200`, `1'200`): `1.2.3`, `192.168.1.1` or a phone number
/// like `030 1234567` are no numbers. A single `,` or `.` is always the
/// decimal separator (`1,200` is 1.2, as `1.200` is).
fn point_form(s: &str) -> Option<(String, bool)> {
    let s = s.trim().trim_start_matches('+');
    if s.is_empty() || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let (sign, body) = s.strip_prefix('-').map_or(("", s), |b| ("-", b.trim_start()));
    // `1 200` and `1'200`: the space or apostrophe groups the whole part.
    let body = match [' ', '\''].into_iter().find(|sep| body.contains(*sep)) {
        Some(sep) => {
            let whole = body.find([',', '.']).unwrap_or(body.len());
            if !grouped(&body[..whole], sep) || body[whole..].contains([' ', '\'']) {
                return None;
            }
            format!("{}{}", body[..whole].replace(sep, ""), &body[whole..])
        }
        None => body.to_string(),
    };
    let commas = body.matches(',').count();
    let dots = body.matches('.').count();
    // The whole part with `sep` between thousands, then the decimals after `decimal`.
    let both = |sep: char, decimal: char| -> Option<String> {
        let (whole, fraction) = body.split_once(decimal)?;
        (grouped(whole, sep) && !fraction.contains([sep, decimal])).then(|| format!("{}.{fraction}", whole.replace(sep, "")))
    };
    let (number, comma_decimal) = match (commas, dots) {
        (0, 0) => (body.clone(), false),
        (c, d) if c > 0 && d > 0 => {
            if body.rfind(',') > body.rfind('.') {
                (both('.', ',')?, true)
            } else {
                (both(',', '.')?, false)
            }
        }
        (1, 0) => (body.replace(',', "."), true),
        (_, 0) => (grouped(&body, ',').then(|| body.replace(',', ""))?, false),
        (0, 1) => (body.clone(), false),
        (0, _) => (grouped(&body, '.').then(|| body.replace('.', ""))?, true),
        _ => unreachable!(),
    };
    Some((format!("{sign}{number}"), comma_decimal))
}

/// Whether `sep` splits `s` into groups of thousands: one to three digits,
/// then groups of exactly three.
fn grouped(s: &str, sep: char) -> bool {
    let mut groups = s.split(sep);
    let digits = |g: &str| g.chars().all(|c| c.is_ascii_digit());
    groups.next().is_some_and(|g| (1..=3).contains(&g.len()) && digits(g)) && groups.all(|g| g.len() == 3 && digits(g))
}

/// Parses `2026-10-08` or `08.10.2026`.
pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let parts: Vec<&str>;
    let (y, m, d) = if s.contains('-') {
        parts = s.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 {
            return None;
        }
        (parts[0], parts[1], parts[2])
    } else if s.contains('.') {
        parts = s.split('.').collect();
        if parts.len() != 3 || parts[2].len() != 4 {
            return None;
        }
        (parts[2], parts[1], parts[0])
    } else {
        return None;
    };
    let (y, m, d): (i64, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's civil date algorithms.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(parse_number("1200,00"), Some((1200.0, true)));
        assert_eq!(parse_number("1.200,50"), Some((1200.5, true)));
        assert_eq!(parse_number("1,200.50"), Some((1200.5, false)));
        assert_eq!(parse_number("2.5"), Some((2.5, false)));
        assert_eq!(parse_number("-7"), Some((-7.0, false)));
        assert_eq!(parse_number("abc"), None);
    }

    #[test]
    fn dates() {
        let d = parse_date("2026-10-08").unwrap();
        assert_eq!(parse_date("08.10.2026"), Some(d));
        assert_eq!(Value::Date(d).display(None, false), "2026-10-08");
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2026-02-29"), None);
    }
}
