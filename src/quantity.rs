//! Physical quantities with SI dimensions, for calculation sheets.
//!
//! A [`Quantity`] holds its value in SI base units and its [`Dimension`]
//! (exponents of m, kg, s, A, K, mol, cd), so `kN + m` is an error and
//! `kN + N` works. Besides that it remembers the unit it should be shown in
//! – a product of unit symbols ([`Unit`]) such as `kN/m^2`. Multiplying and
//! dividing combines those symbols (`kN/m^2 * m` → `kN/m`); where that would
//! mix symbols of one kind (`kN/m^2 * mm`) the preferred unit of the
//! dimension is used instead (force per length → `kN/m`, stress → `N/mm²`).
//!
//! The unit table is construction-oriented: kN, kN/m, kN/m², MPa = N/mm²,
//! kNm, ‰, °, % besides the SI units.

use crate::units::Unit;

/// Exponents of the seven SI base units: m, kg, s, A, K, mol, cd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dimension(pub [i8; 7]);

const BASE: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

impl Dimension {
    pub const NONE: Dimension = Dimension([0; 7]);
    pub const LENGTH: Dimension = Dimension([1, 0, 0, 0, 0, 0, 0]);
    pub const MASS: Dimension = Dimension([0, 1, 0, 0, 0, 0, 0]);
    pub const TIME: Dimension = Dimension([0, 0, 1, 0, 0, 0, 0]);
    pub const FORCE: Dimension = Dimension([1, 1, -2, 0, 0, 0, 0]);
    pub const PRESSURE: Dimension = Dimension([-1, 1, -2, 0, 0, 0, 0]);
    pub const ENERGY: Dimension = Dimension([2, 1, -2, 0, 0, 0, 0]);

    pub fn is_none(self) -> bool {
        self == Dimension::NONE
    }

    pub fn powi(self, n: i32) -> Dimension {
        Dimension(std::array::from_fn(|i| (self.0[i] as i32 * n) as i8))
    }

    /// The n-th root; only when every exponent is divisible by n.
    pub fn root(self, n: i32) -> Option<Dimension> {
        self.0.iter().all(|e| *e as i32 % n == 0).then(|| Dimension(std::array::from_fn(|i| (self.0[i] as i32 / n) as i8)))
    }
}

impl std::ops::Mul for Dimension {
    type Output = Dimension;
    fn mul(self, o: Dimension) -> Dimension {
        Dimension(std::array::from_fn(|i| self.0[i] + o.0[i]))
    }
}

impl std::ops::Div for Dimension {
    type Output = Dimension;
    fn div(self, o: Dimension) -> Dimension {
        Dimension(std::array::from_fn(|i| self.0[i] - o.0[i]))
    }
}

impl std::fmt::Display for Dimension {
    /// In SI base units, e.g. `kg/(m*s^2)` is written `m^-1*kg*s^-2`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_none() {
            return write!(f, "1");
        }
        let parts: Vec<String> = BASE
            .iter()
            .zip(self.0)
            .filter(|(_, e)| *e != 0)
            .map(|(b, e)| if e == 1 { b.to_string() } else { format!("{b}^{e}") })
            .collect();
        write!(f, "{}", parts.join("*"))
    }
}

/// One entry of the unit table: symbol, size in SI units, dimension.
struct UnitDef {
    symbol: &'static str,
    factor: f64,
    dim: [i8; 7],
}

const fn u(symbol: &'static str, factor: f64, dim: [i8; 7]) -> UnitDef {
    UnitDef { symbol, factor, dim }
}

const L: [i8; 7] = [1, 0, 0, 0, 0, 0, 0];
const M: [i8; 7] = [0, 1, 0, 0, 0, 0, 0];
const T: [i8; 7] = [0, 0, 1, 0, 0, 0, 0];
const F: [i8; 7] = [1, 1, -2, 0, 0, 0, 0];
const P: [i8; 7] = [-1, 1, -2, 0, 0, 0, 0];
const E: [i8; 7] = [2, 1, -2, 0, 0, 0, 0];
const W: [i8; 7] = [2, 1, -3, 0, 0, 0, 0];
const ONE: [i8; 7] = [0; 7];

const UNITS: &[UnitDef] = &[
    u("m", 1.0, L),
    u("mm", 1e-3, L),
    u("cm", 1e-2, L),
    u("dm", 1e-1, L),
    u("km", 1e3, L),
    u("kg", 1.0, M),
    u("g", 1e-3, M),
    u("t", 1e3, M),
    u("s", 1.0, T),
    u("ms", 1e-3, T),
    u("min", 60.0, T),
    u("h", 3600.0, T),
    u("d", 86400.0, T),
    u("A", 1.0, [0, 0, 0, 1, 0, 0, 0]),
    u("K", 1.0, [0, 0, 0, 0, 1, 0, 0]),
    u("mol", 1.0, [0, 0, 0, 0, 0, 1, 0]),
    u("cd", 1.0, [0, 0, 0, 0, 0, 0, 1]),
    u("N", 1.0, F),
    u("kN", 1e3, F),
    u("MN", 1e6, F),
    u("Pa", 1.0, P),
    u("kPa", 1e3, P),
    u("MPa", 1e6, P),
    u("GPa", 1e9, P),
    u("bar", 1e5, P),
    u("J", 1.0, E),
    u("kJ", 1e3, E),
    u("Nm", 1.0, E),
    u("kNm", 1e3, E),
    u("MNm", 1e6, E),
    u("kWh", 3.6e6, E),
    u("W", 1.0, W),
    u("kW", 1e3, W),
    u("Hz", 1.0, [0, 0, -1, 0, 0, 0, 0]),
    u("l", 1e-3, [3, 0, 0, 0, 0, 0, 0]),
    u("L", 1e-3, [3, 0, 0, 0, 0, 0, 0]),
    u("%", 1e-2, ONE),
    u("‰", 1e-3, ONE),
    u("°", std::f64::consts::PI / 180.0, ONE),
    u("rad", 1.0, ONE),
];

fn lookup(symbol: &str) -> Option<&'static UnitDef> {
    UNITS.iter().find(|d| d.symbol == symbol)
}

/// Whether `s` is a symbol of the unit table.
pub fn is_unit_symbol(s: &str) -> bool {
    lookup(s).is_some()
}

/// Parses a unit as written by people: `kN/m²`, `N/mm^2`, `kN·m`, `1/s`,
/// `s⁻¹`. Superscripts and `·` are accepted besides `^` and `*`.
pub fn parse_unit(s: &str) -> Option<Unit> {
    let t = s.replace(' ', "");
    // `1/s`: nothing in the numerator.
    if let Some(den) = t.strip_prefix("1/") {
        return Unit::parse(den).map(|d| Unit::none().div(&d));
    }
    Unit::parse(&t)
}

/// Size (in SI units) and dimension of a product of unit symbols.
/// `Err` names the first unknown symbol.
pub fn resolve(unit: &Unit) -> Result<(f64, Dimension), String> {
    let mut factor = 1.0;
    let mut dim = Dimension::NONE;
    for (sym, e) in unit.factors() {
        let def = lookup(sym).ok_or_else(|| format!("unknown unit {sym}"))?;
        factor *= def.factor.powi(e);
        dim = dim * Dimension(def.dim).powi(e);
    }
    Ok((factor, dim))
}

/// The unit a dimension is shown in when nothing else is known.
pub fn preferred(dim: Dimension) -> Option<Unit> {
    let s = match dim.0 {
        [0, 0, 0, 0, 0, 0, 0] => return None,
        [1, 0, 0, 0, 0, 0, 0] => "m",
        [2, 0, 0, 0, 0, 0, 0] => "m^2",
        [3, 0, 0, 0, 0, 0, 0] => "m^3",
        [4, 0, 0, 0, 0, 0, 0] => "cm^4",
        [0, 1, 0, 0, 0, 0, 0] => "kg",
        [0, 0, 1, 0, 0, 0, 0] => "s",
        [1, 1, -2, 0, 0, 0, 0] => "kN",
        [0, 1, -2, 0, 0, 0, 0] => "kN/m",
        [-1, 1, -2, 0, 0, 0, 0] => "N/mm^2",
        [-2, 1, -2, 0, 0, 0, 0] => "kN/m^3",
        [2, 1, -2, 0, 0, 0, 0] => "kNm",
        [2, 1, -3, 0, 0, 0, 0] => "kW",
        [0, 0, -1, 0, 0, 0, 0] => "Hz",
        _ => {
            // Anything else in SI base units.
            let mut unit = Unit::none();
            for (b, e) in BASE.iter().zip(dim.0) {
                unit = unit.mul(&Unit::symbol(b).powi(e as i32));
            }
            return Some(unit);
        }
    };
    Unit::parse(s)
}

/// `kN/m^2` written with superscripts: `kN/m²`.
pub fn pretty(unit: &Unit) -> String {
    let sup = |e: i32| -> String {
        e.to_string()
            .chars()
            .map(|c| match c {
                '-' => '⁻',
                '0' => '⁰',
                '1' => '¹',
                '2' => '²',
                '3' => '³',
                '4' => '⁴',
                '5' => '⁵',
                '6' => '⁶',
                '7' => '⁷',
                '8' => '⁸',
                _ => '⁹',
            })
            .collect()
    };
    let part = |s: &str, e: i32| if e == 1 { s.to_string() } else { format!("{s}{}", sup(e)) };
    let num: Vec<String> = unit.factors().filter(|(_, e)| *e > 0).map(|(s, e)| part(s, e)).collect();
    let den: Vec<String> = unit.factors().filter(|(_, e)| *e < 0).map(|(s, e)| part(s, -e)).collect();
    let num = if num.is_empty() { "1".to_string() } else { num.join("·") };
    match den.len() {
        0 => num,
        1 => format!("{num}/{}", den[0]),
        _ => format!("{num}/({})", den.join("·")),
    }
}

/// A value with dimension, stored in SI units, with the unit to show it in.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    /// In SI base units.
    pub value: f64,
    pub dim: Dimension,
    /// Unit to show the value in; `None`: the preferred one of the dimension.
    pub display: Option<Unit>,
}

impl Quantity {
    pub fn number(value: f64) -> Quantity {
        Quantity { value, dim: Dimension::NONE, display: None }
    }

    /// `value` given in `unit`, e.g. `Quantity::new(84.0, "kN/m")`.
    pub fn new(value: f64, unit: &str) -> Result<Quantity, String> {
        let u = parse_unit(unit).ok_or_else(|| format!("not a unit: {unit}"))?;
        Quantity::with_unit(value, u)
    }

    pub fn with_unit(value: f64, unit: Unit) -> Result<Quantity, String> {
        if unit.is_none() {
            return Ok(Quantity::number(value));
        }
        let (factor, dim) = resolve(&unit)?;
        Ok(Quantity { value: value * factor, dim, display: Some(unit) })
    }

    /// The unit the value is shown in (none for pure numbers).
    pub fn unit(&self) -> Option<Unit> {
        self.display.clone().or_else(|| preferred(self.dim))
    }

    /// The value in the unit it is shown in, and that unit.
    pub fn shown(&self) -> (f64, Option<Unit>) {
        match self.unit() {
            Some(u) => {
                let factor = resolve(&u).map(|(f, _)| f).unwrap_or(1.0);
                (self.value / factor, Some(u))
            }
            None => (self.value, None),
        }
    }

    /// The same quantity, shown in `unit` (`σ = N/A → N/mm²`).
    pub fn convert(&self, unit: &Unit) -> Result<Quantity, String> {
        let (_, dim) = resolve(unit)?;
        if dim != self.dim {
            return Err(format!("cannot show {} in {}", self.describe(), pretty(unit)));
        }
        Ok(Quantity { display: Some(unit.clone()), ..self.clone() })
    }

    fn describe(&self) -> String {
        match self.unit() {
            Some(u) => pretty(&u),
            None => "a number".to_string(),
        }
    }

    pub fn add(&self, o: &Quantity) -> Result<Quantity, String> {
        self.same_dim(o, "+")?;
        Ok(Quantity { value: self.value + o.value, dim: self.dim, display: self.display.clone().or_else(|| o.display.clone()) })
    }

    pub fn sub(&self, o: &Quantity) -> Result<Quantity, String> {
        self.same_dim(o, "-")?;
        Ok(Quantity { value: self.value - o.value, dim: self.dim, display: self.display.clone().or_else(|| o.display.clone()) })
    }

    pub fn neg(&self) -> Quantity {
        Quantity { value: -self.value, ..self.clone() }
    }

    pub fn mul(&self, o: &Quantity) -> Quantity {
        let dim = self.dim * o.dim;
        Quantity { value: self.value * o.value, dim, display: combine(&self.display, &o.display, false, dim) }
    }

    pub fn div(&self, o: &Quantity) -> Result<Quantity, String> {
        if o.value == 0.0 {
            return Err("division by zero".into());
        }
        let dim = self.dim / o.dim;
        Ok(Quantity { value: self.value / o.value, dim, display: combine(&self.display, &o.display, true, dim) })
    }

    /// `self ^ o`: the exponent must be a number; with a dimension, a whole
    /// number (or the inverse of one whose root exists: `A^0,5`).
    pub fn pow(&self, o: &Quantity) -> Result<Quantity, String> {
        if !o.dim.is_none() {
            return Err(format!("exponent with unit {}", o.describe()));
        }
        let value = self.value.powf(o.value);
        if self.dim.is_none() {
            return Ok(Quantity { value, dim: Dimension::NONE, display: None });
        }
        let n = o.value;
        if n.fract() == 0.0 {
            let dim = self.dim.powi(n as i32);
            let display = self.display.as_ref().map(|d| d.powi(n as i32));
            return Ok(Quantity { value, dim, display: tidy(display, dim) });
        }
        let inv = 1.0 / n;
        if (inv - inv.round()).abs() < 1e-9 {
            let r = inv.round() as i32;
            if let Some(dim) = self.dim.root(r) {
                let display = self.display.as_ref().and_then(|d| d.root(r));
                return Ok(Quantity { value, dim, display: tidy(display, dim) });
            }
        }
        Err(format!("{} ^ {n} has no unit", self.describe()))
    }

    pub fn sqrt(&self) -> Result<Quantity, String> {
        if self.value < 0.0 {
            return Err("square root of a negative value".into());
        }
        self.pow(&Quantity::number(0.5))
    }

    /// Compares two quantities of the same dimension.
    pub fn cmp(&self, o: &Quantity, op: &str) -> Result<std::cmp::Ordering, String> {
        self.same_dim(o, op)?;
        self.value.partial_cmp(&o.value).ok_or_else(|| "not a number".to_string())
    }

    fn same_dim(&self, o: &Quantity, op: &str) -> Result<(), String> {
        if self.dim == o.dim {
            Ok(())
        } else {
            Err(format!("units do not match: {} {op} {}", self.describe(), o.describe()))
        }
    }

    /// The value in its unit, 3 significant digits (`1,24 kN/m²`).
    pub fn format(&self, comma: bool) -> String {
        let (v, u) = self.shown();
        let n = format_sig(v, 3, comma);
        match u {
            Some(u) => format!("{n} {}", pretty(&u)),
            None => n,
        }
    }
}

/// Shown unit of a product or quotient: the symbols combined, unless that
/// leaves two symbols of one kind (`kN·mm/m²`) – then the preferred unit.
fn combine(a: &Option<Unit>, b: &Option<Unit>, divide: bool, dim: Dimension) -> Option<Unit> {
    let b = match (b, divide) {
        (Some(b), true) => Some(Unit::none().div(b)),
        (b, _) => b.clone(),
    };
    let joined = match (a, b) {
        (Some(a), Some(b)) => Some(a.mul(&b)),
        (Some(a), None) => Some(a.clone()),
        (None, b) => b,
    };
    tidy(joined, dim)
}

fn tidy(unit: Option<Unit>, dim: Dimension) -> Option<Unit> {
    let unit = unit.filter(|u| !u.is_none())?;
    if dim.is_none() {
        // A pure ratio keeps its unit only when it is one of %, ‰, °.
        let ratio = unit.factors().all(|(s, e)| e == 1 && matches!(s, "%" | "‰" | "°" | "rad"));
        return ratio.then_some(unit);
    }
    // Two symbols of the same base dimension that did not cancel.
    let kinds: Vec<[i8; 7]> = unit.factors().filter_map(|(s, _)| lookup(s).map(|d| d.dim)).collect();
    let mixed = (0..kinds.len()).any(|i| (i + 1..kinds.len()).any(|j| kinds[i] == kinds[j]));
    if mixed || resolve(&unit).is_err() { preferred(dim) } else { Some(unit) }
}

/// `n` with `sig` significant digits, at least the integer part; trailing
/// zeros stay (`60,0`).
pub fn format_sig(v: f64, sig: i32, comma: bool) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v == 0.0 { "0".into() } else { v.to_string() };
    }
    let mag = v.abs().log10().floor() as i32;
    let decimals = (sig - 1 - mag).max(0) as usize;
    let s = format!("{:.*}", decimals, crate::value::round_to(v, decimals as i32));
    if comma { s.replace('.', ",") } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(v: f64, u: &str) -> Quantity {
        Quantity::new(v, u).unwrap()
    }

    #[test]
    fn units_with_superscripts() {
        let u = parse_unit("kN/m²").unwrap();
        assert_eq!(u, Unit::parse("kN/m^2").unwrap());
        assert_eq!(pretty(&u), "kN/m²");
        assert_eq!(parse_unit("s⁻¹").unwrap(), parse_unit("1/s").unwrap());
        assert_eq!(pretty(&parse_unit("kN·m").unwrap()), "kN·m");
    }

    #[test]
    fn dimensions_checked() {
        let e = q(1.0, "kN").add(&q(1.0, "m")).unwrap_err();
        assert_eq!(e, "units do not match: kN + m");
        let s = q(1.0, "kN").add(&q(500.0, "N")).unwrap();
        assert_eq!(s.format(true), "1,50 kN");
    }

    #[test]
    fn mpa_is_n_per_mm2() {
        let a = q(1.0, "MPa");
        let b = q(1.0, "N/mm^2");
        assert_eq!(a.dim, b.dim);
        assert!((a.value - b.value).abs() < 1e-6);
    }

    #[test]
    fn display_units_combine_and_cancel() {
        let line = q(2.0, "kN/m^2").mul(&q(3.0, "m"));
        assert_eq!(line.format(true), "6,00 kN/m");
        // kN·mm/m² mixes lengths: the preferred unit of force per length.
        let mixed = q(2.0, "kN/m^2").mul(&q(3000.0, "mm"));
        assert_eq!(mixed.format(true), "6,00 kN/m");
        let f_d = q(84.0, "kN/m").div(&Quantity::number(1.4)).unwrap();
        assert_eq!(f_d.format(true), "60,0 kN/m");
    }

    #[test]
    fn preferred_units() {
        let sigma = q(100.0, "kN").div(&q(50.0, "cm^2")).unwrap();
        assert_eq!(sigma.format(false), "2.00 kN/cm²");
        let sigma = Quantity { display: None, ..sigma };
        assert_eq!(sigma.format(false), "20.0 N/mm²");
        let m = q(10.0, "kN").mul(&q(2.0, "m"));
        assert_eq!(m.format(false), "20.0 kN·m");
        let m = Quantity { display: None, ..m };
        assert_eq!(m.format(false), "20.0 kNm");
    }

    #[test]
    fn explicit_conversion() {
        let sigma = q(100.0, "kN").div(&q(50.0, "cm^2")).unwrap();
        let in_mpa = sigma.convert(&parse_unit("MPa").unwrap()).unwrap();
        assert_eq!(in_mpa.format(false), "20.0 MPa");
        assert!(sigma.convert(&parse_unit("m").unwrap()).is_err());
    }

    #[test]
    fn ratios_and_powers() {
        assert_eq!(q(5.0, "%").value, 0.05);
        assert_eq!(q(1.0, "kN").div(&q(2.0, "kN")).unwrap().format(true), "0,500");
        let area = q(3.0, "m").pow(&Quantity::number(2.0)).unwrap();
        assert_eq!(area.format(true), "9,00 m²");
        assert_eq!(area.sqrt().unwrap().format(true), "3,00 m");
        assert!(q(2.0, "m").pow(&Quantity::number(0.5)).is_err());
    }

    #[test]
    fn unknown_units() {
        assert_eq!(Quantity::new(1.0, "€").unwrap_err(), "unknown unit €");
    }

    #[test]
    fn significant_digits() {
        assert_eq!(format_sig(1.2375, 3, true), "1,24");
        assert_eq!(format_sig(38.2, 3, true), "38,2");
        assert_eq!(format_sig(60.0, 3, true), "60,0");
        assert_eq!(format_sig(1234.4, 3, false), "1234");
        assert_eq!(format_sig(0.0123, 3, false), "0.0123");
    }
}
