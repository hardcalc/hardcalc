//! Physical / monetary units as products of base symbols with integer exponents.
//!
//! `m/s^2` is `{m: 1, s: -2}`. Units are compared structurally; there is no
//! conversion between symbols yet (`km` and `m` are unrelated).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unit(BTreeMap<String, i32>);

impl Unit {
    pub fn none() -> Self {
        Unit::default()
    }

    pub fn symbol(s: &str) -> Self {
        Unit(BTreeMap::from([(s.to_string(), 1)]))
    }

    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }

    pub fn mul(&self, other: &Unit) -> Unit {
        let mut m = self.0.clone();
        for (k, e) in &other.0 {
            *m.entry(k.clone()).or_insert(0) += e;
        }
        m.retain(|_, e| *e != 0);
        Unit(m)
    }

    pub fn div(&self, other: &Unit) -> Unit {
        self.mul(&other.powi(-1))
    }

    pub fn powi(&self, n: i32) -> Unit {
        let mut m: BTreeMap<String, i32> = self.0.iter().map(|(k, e)| (k.clone(), e * n)).collect();
        m.retain(|_, e| *e != 0);
        Unit(m)
    }

    /// Square root; only defined when every exponent is even.
    pub fn sqrt(&self) -> Option<Unit> {
        self.root(2)
    }

    /// The n-th root; only defined when every exponent is divisible by n.
    pub fn root(&self, n: i32) -> Option<Unit> {
        self.0
            .iter()
            .map(|(k, e)| (e % n == 0).then(|| (k.clone(), e / n)))
            .collect::<Option<BTreeMap<_, _>>>()
            .map(Unit)
    }

    /// The symbols and their exponents.
    pub fn factors(&self) -> impl Iterator<Item = (&str, i32)> {
        self.0.iter().map(|(k, e)| (k.as_str(), *e))
    }

    /// Parses `€`, `m/s`, `kg*m/s^2`, `m^2`, `W/(m^2*K)`; superscripts (`cm³`,
    /// `s⁻¹`) and `⋅` are read as `^` and `*`. Operators work from left to
    /// right: `a/b*c` is `a·c/b`; parentheses group. `None` for malformed input.
    pub fn parse(s: &str) -> Option<Unit> {
        let s = carets(s.trim());
        let chars: Vec<char> = s.chars().collect();
        let mut pos = 0;
        let unit = parse_product(&chars, &mut pos)?;
        (pos == chars.len()).then_some(unit)
    }
}

/// `a/b*c`, up to the end or a closing parenthesis.
fn parse_product(c: &[char], pos: &mut usize) -> Option<Unit> {
    let mut unit = Unit::none();
    let mut sign = 1;
    loop {
        let factor = parse_factor(c, pos)?;
        unit = unit.mul(&factor.powi(sign));
        match c.get(*pos) {
            Some(op @ ('*' | '/' | '·')) => {
                sign = if *op == '/' { -1 } else { 1 };
                *pos += 1;
            }
            _ => return Some(unit),
        }
    }
}

/// A symbol or a parenthesised product, with an optional `^exponent`.
fn parse_factor(c: &[char], pos: &mut usize) -> Option<Unit> {
    while c.get(*pos).is_some_and(|ch| ch.is_whitespace()) {
        *pos += 1;
    }
    let base = if c.get(*pos) == Some(&'(') {
        *pos += 1;
        let inner = parse_product(c, pos)?;
        if c.get(*pos) != Some(&')') {
            return None;
        }
        *pos += 1;
        inner
    } else {
        let start = *pos;
        while c.get(*pos).is_some_and(|ch| !matches!(ch, '*' | '/' | '·' | '^' | '(' | ')')) {
            *pos += 1;
        }
        let sym: String = c[start..*pos].iter().collect();
        let sym = sym.trim();
        if sym.is_empty() || sym.chars().any(|ch| ch.is_ascii_digit() || ch.is_whitespace()) {
            return None;
        }
        Unit::symbol(sym)
    };
    while c.get(*pos).is_some_and(|ch| ch.is_whitespace()) {
        *pos += 1;
    }
    if c.get(*pos) != Some(&'^') {
        return Some(base);
    }
    *pos += 1;
    let start = *pos;
    if c.get(*pos) == Some(&'-') {
        *pos += 1;
    }
    while c.get(*pos).is_some_and(|ch| ch.is_ascii_digit()) {
        *pos += 1;
    }
    let exp: i32 = c[start..*pos].iter().collect::<String>().parse().ok()?;
    Some(base.powi(exp))
}

/// `cm³` → `cm^3`, `s⁻¹` → `s^-1`, `kN⋅m` → `kN*m`.
fn carets(s: &str) -> String {
    let mut t = String::new();
    let mut in_sup = false;
    for ch in s.chars() {
        let sup = match ch {
            '⁰' => Some('0'),
            '¹' => Some('1'),
            '²' => Some('2'),
            '³' => Some('3'),
            '⁴' => Some('4'),
            '⁵' => Some('5'),
            '⁶' => Some('6'),
            '⁷' => Some('7'),
            '⁸' => Some('8'),
            '⁹' => Some('9'),
            '⁻' => Some('-'),
            _ => None,
        };
        match sup {
            Some(d) => {
                if !in_sup {
                    t.push('^');
                    in_sup = true;
                }
                t.push(d);
            }
            None => {
                in_sup = false;
                t.push(if ch == '⋅' { '*' } else { ch });
            }
        }
    }
    t
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let part = |sym: &str, e: i32| if e == 1 { sym.to_string() } else { format!("{sym}^{e}") };
        let num: Vec<String> = self.0.iter().filter(|(_, e)| **e > 0).map(|(s, e)| part(s, *e)).collect();
        let den: Vec<String> = self.0.iter().filter(|(_, e)| **e < 0).map(|(s, e)| part(s, -e)).collect();
        let num = if num.is_empty() { "1".to_string() } else { num.join("*") };
        if den.is_empty() {
            write!(f, "{num}")
        } else if den.len() > 1 {
            write!(f, "{num}/({})", den.join("*"))
        } else {
            write!(f, "{num}/{}", den.join("*"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        assert_eq!(Unit::parse("m/s^2").unwrap().to_string(), "m/s^2");
        assert_eq!(Unit::parse("kg*m/s^2").unwrap().to_string(), "kg*m/s^2");
        assert_eq!(Unit::parse("€").unwrap().to_string(), "€");
        assert!(Unit::parse("5").is_none());
        assert_eq!(Unit::parse("cm³"), Unit::parse("cm^3"));
        assert_eq!(Unit::parse("kN/m²"), Unit::parse("kN/m^2"));
        assert_eq!(Unit::parse("s⁻¹"), Unit::parse("s^-1"));
        assert_eq!(Unit::parse("W/(m^2*K)"), Unit::parse("W/m^2/K"));
        assert_eq!(Unit::parse("W/(m²·K)").unwrap().to_string(), "W/(K*m^2)");
        assert_eq!(Unit::parse("(kN*m)^2/m"), Unit::parse("kN^2*m"));
        assert!(Unit::parse("W/(m^2").is_none());
        assert!(Unit::parse("W/m)").is_none());
    }

    #[test]
    fn arithmetic_cancels() {
        let v = Unit::parse("m/s").unwrap();
        let t = Unit::symbol("s");
        assert_eq!(v.mul(&t), Unit::symbol("m"));
        assert!(Unit::symbol("€").div(&Unit::symbol("€")).is_none());
    }
}
