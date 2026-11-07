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
fn point_form(s: &str) -> Option<(String, bool)> {
    let s: String = s.trim().trim_start_matches('+').chars().filter(|c| *c != ' ' && *c != '\'').collect();
    if s.is_empty() || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let commas = s.matches(',').count();
    let dots = s.matches('.').count();
    Some(match (commas, dots) {
        (0, 0) => (s.clone(), false),
        (c, d) if c > 0 && d > 0 => {
            if s.rfind(',') > s.rfind('.') {
                (s.replace('.', "").replace(',', "."), true)
            } else {
                (s.replace(',', ""), false)
            }
        }
        (1, 0) => (s.replace(',', "."), true),
        (_, 0) => (s.replace(',', ""), false),
        (0, 1) => (s.clone(), false),
        (0, _) => (s.replace('.', ""), true),
        _ => unreachable!(),
    })
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
