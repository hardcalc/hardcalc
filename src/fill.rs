//! Filling like Excel's fill handle: continues a series from the cells it
//! starts with – numbers, dates, "Item 1", month and weekday names – or
//! repeats them (formulas are copied with their references shifted by the
//! caller).

use crate::value::{civil_from_days, parse_date, parse_number};

/// What goes into a filled cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Text(String),
    /// A copy of source cell `i` (a formula is shifted to its new place).
    From(usize),
}

const LISTS: &[&[&str]] = &[
    &["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    &["January", "February", "March", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    &["Jan", "Feb", "Mar", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
    &["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
    &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    &["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"],
    &["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"],
];

/// Step between the first and last of `values` (evenly spread).
fn step(values: &[f64]) -> f64 {
    if values.len() < 2 { 1.0 } else { (values[values.len() - 1] - values[0]) / (values.len() - 1) as f64 }
}

fn decimals(raw: &str) -> usize {
    raw.trim().rsplit_once(['.', ',']).map_or(0, |(_, d)| if d.chars().all(|c| c.is_ascii_digit()) { d.len() } else { 0 })
}

/// `count` cells continuing `source` (as written in the cells).
pub fn series(source: &[String], count: usize) -> Vec<Fill> {
    let n = source.len();
    let repeat = || (0..count).map(|k| Fill::From(k % n)).collect();
    if n == 0 || source.iter().any(|s| s.trim().is_empty() || s.trim_start().starts_with('=')) {
        return if n == 0 { Vec::new() } else { repeat() };
    }
    // Dates first: 01.10.2026 would also read as a number.
    if let Some(days) = source.iter().map(|s| parse_date(s)).collect::<Option<Vec<i64>>>() {
        let d = step(&days.iter().map(|&d| d as f64).collect::<Vec<_>>()).round() as i64;
        let german = source[n - 1].contains('.');
        return (1..=count as i64)
            .map(|k| {
                let (y, m, day) = civil_from_days(days[n - 1] + d * k);
                Fill::Text(if german { format!("{day:02}.{m:02}.{y:04}") } else { format!("{y:04}-{m:02}-{day:02}") })
            })
            .collect();
    }
    if let Some(nums) = source.iter().map(|s| parse_number(s)).collect::<Option<Vec<_>>>() {
        // A single number is copied, as in Excel.
        if n == 1 {
            return repeat();
        }
        let comma = nums.iter().any(|(_, c)| *c);
        let values: Vec<f64> = nums.iter().map(|(v, _)| *v).collect();
        let d = step(&values);
        let places = source.iter().map(|s| decimals(s)).max().unwrap_or(0);
        return (1..=count)
            .map(|k| {
                let s = format!("{:.*}", places, values[n - 1] + d * k as f64);
                Fill::Text(if comma { s.replace('.', ",") } else { s })
            })
            .collect();
    }
    // Month and weekday names, in the case of the first one.
    for list in LISTS {
        let index = |s: &String| list.iter().position(|x| x.eq_ignore_ascii_case(s.trim()));
        if let Some(found) = source.iter().map(index).collect::<Option<Vec<usize>>>() {
            let len = list.len() as isize;
            let d = if n < 2 { 1 } else { (found[n - 1] as isize - found[n - 2] as isize).rem_euclid(len) };
            let first = source[0].trim();
            return (1..=count as isize)
                .map(|k| {
                    let name = list[(found[n - 1] as isize + d * k).rem_euclid(len) as usize];
                    Fill::Text(if first.chars().all(|c| !c.is_lowercase()) && first.chars().count() > 1 {
                        name.to_uppercase()
                    } else if first.chars().next().is_some_and(char::is_lowercase) {
                        name.to_lowercase()
                    } else {
                        name.to_string()
                    })
                })
                .collect();
        }
    }
    // Text with a number at the end: "Article 1", "Q1", "Nr. 007".
    let split = |s: &String| {
        let digits = s.chars().rev().take_while(char::is_ascii_digit).count();
        (digits > 0).then(|| {
            let at = s.len() - digits;
            (s[..at].to_string(), s[at..].to_string())
        })
    };
    if let Some(parts) = source.iter().map(split).collect::<Option<Vec<_>>>()
        && parts.iter().all(|(p, _)| *p == parts[0].0)
    {
        let values: Vec<f64> = parts.iter().map(|(_, d)| d.parse::<f64>().unwrap_or(0.0)).collect();
        let d = step(&values).round() as i64;
        let width = parts[n - 1].1.len();
        let last = values[n - 1] as i64;
        return (1..=count as i64).map(|k| Fill::Text(format!("{}{:0width$}", parts[0].0, (last + d * k).max(0)))).collect();
    }
    repeat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &[&str], count: usize) -> Vec<String> {
        let source: Vec<String> = source.iter().map(|s| s.to_string()).collect();
        series(&source, count)
            .into_iter()
            .map(|f| match f {
                Fill::Text(t) => t,
                Fill::From(i) => format!("<{}>", source[i]),
            })
            .collect()
    }

    #[test]
    fn series_continue() {
        assert_eq!(texts(&["1", "2"], 3), ["3", "4", "5"]);
        assert_eq!(texts(&["10", "8"], 2), ["6", "4"]);
        assert_eq!(texts(&["0,5", "1,0"], 2), ["1,5", "2,0"]);
        assert_eq!(texts(&["7"], 2), ["<7>", "<7>"], "a single number is copied");
        assert_eq!(texts(&["2026-10-30"], 3), ["2026-10-31", "2026-11-01", "2026-11-02"]);
        assert_eq!(texts(&["01.01.2026", "08.01.2026"], 1), ["15.01.2026"]);
        assert_eq!(texts(&["January"], 2), ["February", "March"]);
        assert_eq!(texts(&["Nov", "Dec"], 2), ["Jan", "Feb"]);
        assert_eq!(texts(&["MO", "MI"], 2), ["FR", "SO"]);
        assert_eq!(texts(&["Article 1"], 2), ["Article 2", "Article 3"]);
        assert_eq!(texts(&["Nr. 007", "Nr. 009"], 1), ["Nr. 011"]);
        assert_eq!(texts(&["a", "b"], 3), ["<a>", "<b>", "<a>"]);
        assert_eq!(texts(&["=A1*2", "x"], 2), ["<=A1*2>", "<x>"]);
    }
}
