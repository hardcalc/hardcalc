//! Charts: the ```chart block format and resolving chart data from tables.
//!
//! ```text
//! type:   bar | line | area | scatter | histogram
//! data:   Table.Column[, Table.Column …]   one series per column
//!         Table                             a tensor (with x / series / filter)
//! labels: Table.Column                      category labels (column charts)
//! x:      Axis | Table.Column               tensor axis, or x values for scatter
//! series: Axis                              tensor axis that becomes the series
//! filter: Axis=Value[, Axis=Value …]        fixes the remaining tensor axes
//! bins:   n                                 histogram bins
//! title:  text
//! ```

use crate::engine::Workbook;
use crate::model::Shape;
use crate::units::Unit;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartType {
    Bar,
    Line,
    Area,
    Scatter,
    Histogram,
}

impl ChartType {
    pub const ALL: [ChartType; 5] =
        [ChartType::Bar, ChartType::Line, ChartType::Area, ChartType::Scatter, ChartType::Histogram];

    pub fn name(self) -> &'static str {
        match self {
            ChartType::Bar => "bar",
            ChartType::Line => "line",
            ChartType::Area => "area",
            ChartType::Scatter => "scatter",
            ChartType::Histogram => "histogram",
        }
    }

    fn parse(s: &str) -> Option<ChartType> {
        ChartType::ALL.into_iter().find(|t| t.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartSpec {
    pub name: String,
    pub kind: ChartType,
    pub data: Vec<String>,
    pub labels: Option<String>,
    pub x: Option<String>,
    pub series: Option<String>,
    pub filter: Vec<(String, String)>,
    pub bins: Option<usize>,
    pub title: Option<String>,
}

impl ChartSpec {
    pub fn parse(name: &str, lines: &[String]) -> Result<ChartSpec, String> {
        let mut spec = ChartSpec {
            name: name.to_string(),
            kind: ChartType::Bar,
            data: Vec::new(),
            labels: None,
            x: None,
            series: None,
            filter: Vec::new(),
            bins: None,
            title: None,
        };
        for line in lines {
            // `  # comment` at the end of a line (a `#` inside a value needs a preceding space to count).
            let line = match line.find(" #") {
                Some(i) => &line[..i],
                None => line.as_str(),
            };
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let (key, value) = t.split_once(':').ok_or_else(|| format!("expected `key: value`: {t}"))?;
            let value = value.trim().to_string();
            let list = || value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
            match key.trim() {
                "type" => spec.kind = ChartType::parse(&value).ok_or_else(|| format!("unknown chart type '{value}'"))?,
                "data" => spec.data = list(),
                "labels" => spec.labels = Some(value),
                "x" => spec.x = Some(value),
                "series" => spec.series = Some(value),
                "filter" => {
                    spec.filter = list()
                        .into_iter()
                        .map(|f| match f.split_once('=') {
                            Some((a, v)) => Ok((a.trim().to_string(), v.trim().to_string())),
                            None => Err(format!("filter needs Axis=Value: {f}")),
                        })
                        .collect::<Result<_, _>>()?
                }
                "bins" => spec.bins = Some(value.parse().map_err(|_| format!("bins must be a number: {value}"))?),
                "title" => spec.title = Some(value),
                other => return Err(format!("unknown chart property '{other}'")),
            }
        }
        if spec.data.is_empty() {
            return Err("chart needs `data:`".into());
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub values: Vec<Option<f64>>,
}

/// Resolved, ready-to-draw chart data.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartData {
    pub title: String,
    pub kind: ChartType,
    /// One label per data point (x position).
    pub categories: Vec<String>,
    /// Numeric x values (scatter only).
    pub xs: Option<Vec<f64>>,
    pub series: Vec<Series>,
    /// Unit of the values (y axis) and of the x values.
    pub unit: Unit,
    pub x_unit: Unit,
    /// Sheet the data comes from.
    pub sheet: usize,
    /// For column charts: category `i` is row `i` of the sheet.
    pub rows_are_categories: bool,
    /// For tensor charts: the x axis column (to match a table row to a category).
    pub x_column: Option<usize>,
    pub decimal_comma: bool,
}

impl ChartData {
    /// (min, max) over all values, always including 0 for bar-like charts.
    pub fn value_range(&self) -> (f64, f64) {
        let vals = self.series.iter().flat_map(|s| s.values.iter().flatten().copied());
        let (mut lo, mut hi) = vals.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        if !lo.is_finite() {
            return (0.0, 1.0);
        }
        if matches!(self.kind, ChartType::Bar | ChartType::Area | ChartType::Histogram) {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        if lo == hi {
            hi = lo + 1.0;
        }
        (lo, hi)
    }

    /// Compact number text in the document's locale, with unit if `with_unit`.
    pub fn fmt(&self, n: f64, with_unit: bool) -> String {
        let s = if n.fract() == 0.0 && n.abs() < 1e15 {
            format!("{n:.0}")
        } else {
            let s = format!("{n:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        let s = if self.decimal_comma { s.replace('.', ",") } else { s };
        if with_unit && !self.unit.is_none() { format!("{s} {}", self.unit) } else { s }
    }
}

/// `Table!Column` (or `Table.Column`, the older form) → table, column and
/// the character between them.
pub fn split_ref(item: &str) -> Option<(&str, &str, char)> {
    ['!', '.'].into_iter().find_map(|sep| item.split_once(sep).map(|(t, c)| (t, c, sep)))
}

/// `Table!Column` → (sheet, column).
fn column_ref(wb: &Workbook, r: &str) -> Result<(usize, usize), String> {
    let (table, col, _) = split_ref(r).ok_or_else(|| format!("expected Table!Column, got '{r}'"))?;
    let table = table.trim().trim_matches('"');
    let s = wb.sheet_index(table).ok_or_else(|| format!("unknown table '{table}'"))?;
    let col = col.trim();
    let c = wb.sheets[s]
        .columns
        .iter()
        .position(|c| c.name == col)
        .ok_or_else(|| format!("unknown column '{table}.{col}'"))?;
    Ok((s, c))
}

/// Numbers of a column (None for empty / non-numeric cells) and their common unit.
fn column_values(wb: &mut Workbook, (s, c): (usize, usize), unit: &mut Option<Unit>) -> Result<Vec<Option<f64>>, String> {
    let rows = wb.sheets[s].cells.len();
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        match wb.value((s, r, c)) {
            Value::Num(n, u) => {
                match unit {
                    Some(prev) if *prev != u => {
                        return Err(format!("unit mismatch in chart: {} vs {}", unit_name(prev), unit_name(&u)));
                    }
                    _ => *unit = Some(u),
                }
                out.push(Some(n));
            }
            _ => out.push(None),
        }
    }
    Ok(out)
}

fn unit_name(u: &Unit) -> String {
    if u.is_none() { "no unit".into() } else { u.to_string() }
}

pub fn resolve(spec: &ChartSpec, wb: &mut Workbook) -> Result<ChartData, String> {
    let title = spec.title.clone().unwrap_or_else(|| spec.name.clone());
    let base = |sheet: usize, kind: ChartType| ChartData {
        title: title.clone(),
        kind,
        categories: Vec::new(),
        xs: None,
        series: Vec::new(),
        unit: Unit::none(),
        x_unit: Unit::none(),
        sheet,
        rows_are_categories: false,
        x_column: None,
        decimal_comma: wb.decimal_comma,
    };

    // A single bare table name refers to a tensor.
    if let [name] = spec.data.as_slice()
        && !name.contains(['.', '!'])
    {
        return resolve_tensor(spec, wb, name.trim_matches('"'), base(0, spec.kind));
    }

    let refs: Vec<(usize, usize)> = spec.data.iter().map(|r| column_ref(wb, r)).collect::<Result<_, _>>()?;
    let sheet = refs[0].0;
    if refs.iter().any(|(s, _)| *s != sheet) {
        return Err("all data columns must come from the same table".into());
    }
    let mut data = base(sheet, spec.kind);
    let mut unit = None;
    for &(s, c) in &refs {
        let values = column_values(wb, (s, c), &mut unit)?;
        data.series.push(Series { name: wb.sheets[s].columns[c].name.clone(), values });
    }
    data.unit = unit.unwrap_or_default();

    let rows = wb.sheets[sheet].cells.len();
    if spec.kind == ChartType::Histogram {
        return Ok(histogram(data, spec.bins));
    }
    data.categories = match &spec.labels {
        Some(l) => {
            let (s, c) = column_ref(wb, l)?;
            if s != sheet {
                return Err("labels must come from the data table".into());
            }
            (0..rows).map(|r| wb.display((s, r, c))).collect()
        }
        None => (1..=rows).map(|r| r.to_string()).collect(),
    };
    data.rows_are_categories = true;
    if spec.kind == ChartType::Scatter {
        let x = spec.x.as_ref().ok_or("scatter needs `x: Table.Column`")?;
        let (s, c) = column_ref(wb, x)?;
        if s != sheet {
            return Err("x must come from the data table".into());
        }
        let mut xu = None;
        let xs = column_values(wb, (s, c), &mut xu)?;
        data.x_unit = xu.unwrap_or_default();
        // Rows without an x value are dropped from every series.
        data.xs = Some(xs.iter().map(|x| x.unwrap_or(f64::NAN)).collect());
    }
    Ok(data)
}

fn resolve_tensor(spec: &ChartSpec, wb: &mut Workbook, name: &str, mut data: ChartData) -> Result<ChartData, String> {
    let s = wb.sheet_index(name).ok_or_else(|| format!("unknown table '{name}'"))?;
    let Shape::Tensor(axes) = wb.sheets[s].shape.clone() else {
        return Err(format!("'{name}' is not a tensor – use Table!Column for columns"));
    };
    let axis = |a: &str| axes.iter().position(|x| x == a).ok_or_else(|| format!("'{a}' is not an axis of {name} ({})", axes.join(", ")));
    let x = match &spec.x {
        Some(a) => axis(a)?,
        None => 0,
    };
    let series = spec.series.as_deref().map(axis).transpose()?;
    let filters: Vec<(usize, String)> =
        spec.filter.iter().map(|(a, v)| Ok((axis(a)?, v.clone()))).collect::<Result<_, String>>()?;
    let value_col = wb.sheets[s].columns.len() - 1;

    let raw = wb.sheets[s].raw.clone();
    let rows: Vec<usize> = (0..raw.len()).filter(|&r| filters.iter().all(|(a, v)| raw[r][*a].trim() == v)).collect();
    let mut categories: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for &r in &rows {
        let k = raw[r][x].trim().to_string();
        if !categories.contains(&k) {
            categories.push(k);
        }
        let n = series.map(|a| raw[r][a].trim().to_string()).unwrap_or_else(|| wb.sheets[s].columns[value_col].name.clone());
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let mut unit = None;
    let mut out: Vec<Series> = names.iter().map(|n| Series { name: n.clone(), values: vec![None; categories.len()] }).collect();
    for &r in &rows {
        let i = categories.iter().position(|k| k == raw[r][x].trim()).unwrap();
        let j = match series {
            Some(a) => names.iter().position(|n| n == raw[r][a].trim()).unwrap(),
            None => 0,
        };
        if let Value::Num(n, u) = wb.value((s, r, value_col)) {
            match &unit {
                Some(prev) if *prev != u => return Err(format!("unit mismatch in chart: {} vs {}", unit_name(prev), unit_name(&u))),
                _ => unit = Some(u),
            }
            // Several rows with the same keys add up, as in the pivot view.
            let slot = &mut out[j].values[i];
            *slot = Some(slot.unwrap_or(0.0) + n);
        }
    }
    data.sheet = s;
    data.categories = categories;
    data.series = out;
    data.unit = unit.unwrap_or_default();
    data.x_column = Some(x);
    if spec.kind == ChartType::Histogram {
        return Ok(histogram(data, spec.bins));
    }
    Ok(data)
}

/// Turns the values of the first series into bin counts.
fn histogram(mut data: ChartData, bins: Option<usize>) -> ChartData {
    let values: Vec<f64> = data.series.first().map(|s| s.values.iter().flatten().copied().collect()).unwrap_or_default();
    let n = bins.unwrap_or_else(|| (values.len() as f64).sqrt().ceil() as usize).max(1);
    let (lo, hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let (lo, hi) = if lo.is_finite() { (lo, if hi > lo { hi } else { lo + 1.0 }) } else { (0.0, 1.0) };
    let width = (hi - lo) / n as f64;
    let mut counts = vec![0.0; n];
    for v in values {
        let i = (((v - lo) / width) as usize).min(n - 1);
        counts[i] += 1.0;
    }
    let edge = |k: usize| data.fmt(lo + width * k as f64, false);
    data.categories = (0..n).map(|k| format!("{}–{}", edge(k), edge(k + 1))).collect();
    let name = data.series.first().map(|s| s.name.clone()).unwrap_or_default();
    data.series = vec![Series { name: format!("count of {name}"), values: counts.into_iter().map(Some).collect() }];
    data.unit = Unit::none();
    data.rows_are_categories = false;
    data
}

/// `=spark(range)`: a sparkline of the numbers, e.g. `▁▃▅█`.
pub fn sparkline(values: &[f64]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let (lo, hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    values
        .iter()
        .map(|&v| if hi > lo { BARS[(((v - lo) / (hi - lo)) * 7.0).round() as usize] } else { BARS[3] })
        .collect()
}

/// A chart drawn in a calculation sheet (`[Moments] plot(M)`), as the
/// charts of tables are drawn. All curves are shown in the unit of the first.
pub fn from_plot(name: &str, p: &crate::sheet::Plot, decimal_comma: bool) -> Result<ChartData, String> {
    let kind = match p.kind.as_str() {
        "area" => ChartType::Area,
        "bar" => ChartType::Bar,
        "scatter" => ChartType::Scatter,
        _ => ChartType::Line,
    };
    let first = &p.ys[0].1;
    let unit = first.span().and_then(|(_, hi)| hi.shown().1);
    let in_unit = |q: &crate::quantity::Quantity, u: &Option<Unit>| -> Result<f64, String> {
        match u {
            Some(u) => q.convert(u).map(|c| c.shown().0),
            None if q.dim.is_none() => Ok(q.value),
            None => Err("the curves of a chart must be of the same kind".to_string()),
        }
    };
    let mut series = Vec::new();
    for (label, s) in &p.ys {
        let values = s.items.iter().map(|q| in_unit(q, &unit).map(Some)).collect::<Result<_, _>>()?;
        series.push(Series { name: label.clone(), values });
    }
    let n = first.items.len();
    let (categories, xs, x_unit) = match &p.x {
        Some((_, xs)) => {
            let xu = xs.span().and_then(|(_, hi)| hi.shown().1);
            let values: Vec<f64> = xs.items.iter().map(|q| in_unit(q, &xu)).collect::<Result<_, _>>()?;
            let labels = values.iter().map(|v| crate::quantity::format_sig(*v, 3, decimal_comma)).collect();
            (labels, Some(values), xu.unwrap_or_default())
        }
        None => ((1..=n).map(|i| i.to_string()).collect(), None, Unit::none()),
    };
    Ok(ChartData {
        title: p.title.clone().unwrap_or_else(|| name.to_string()),
        kind,
        categories,
        xs,
        series,
        unit: unit.unwrap_or_default(),
        x_unit,
        // Not from a table.
        sheet: usize::MAX,
        rows_are_categories: false,
        x_column: None,
        decimal_comma,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SRC: &str = include_str!("../examples/budget.hcmd");

    fn book() -> Workbook {
        Workbook::from_document(&parse(SRC).unwrap()).0
    }

    fn spec(lines: &[&str]) -> ChartSpec {
        ChartSpec::parse("T", &lines.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn parse_spec() {
        let s = spec(&["type: line  # trend", "data: Sales", "x: Month", "series: Region", "filter: Product=A"]);
        assert_eq!(s.kind, ChartType::Line);
        assert_eq!(s.filter, vec![("Product".to_string(), "A".to_string())]);
        assert!(ChartSpec::parse("T", &["type: pie".into(), "data: A.b".into()]).is_err());
        assert!(ChartSpec::parse("T", &["type: bar".into()]).is_err());
    }

    #[test]
    fn column_chart() {
        let mut wb = book();
        let d = resolve(&spec(&["data: Expenses.Amount, Expenses.Gross", "labels: Expenses.Item"]), &mut wb).unwrap();
        assert_eq!(d.categories, ["Rent", "Power", "Internet"]);
        assert_eq!(d.series.len(), 2);
        assert_eq!(d.series[0].values, [Some(1200.0), Some(85.5), Some(39.99)]);
        assert_eq!(d.unit, Unit::symbol("€"));
        assert!(d.rows_are_categories);
    }

    #[test]
    fn tensor_chart() {
        let mut wb = book();
        let d = resolve(&spec(&["type: line", "data: Sales", "x: Month", "series: Region", "filter: Product=A"]), &mut wb)
            .unwrap();
        assert_eq!(d.categories, ["Jan", "Feb"]);
        let names: Vec<&str> = d.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["North", "South"]);
        assert_eq!(d.series[0].values, [Some(120.0), Some(135.0)]);
        assert_eq!(d.series[1].values, [Some(80.0), None]);
        // Without series and filter all rows of a month add up.
        let d = resolve(&spec(&["data: Sales", "x: Month"]), &mut wb).unwrap();
        assert_eq!(d.series[0].values, [Some(200.0), Some(177.1)]);
    }

    #[test]
    fn errors() {
        let mut wb = book();
        assert!(resolve(&spec(&["data: Expenses.Nope"]), &mut wb).unwrap_err().contains("unknown column"));
        assert!(resolve(&spec(&["data: Expenses"]), &mut wb).unwrap_err().contains("not a tensor"));
        assert!(resolve(&spec(&["data: Sales", "x: Year"]), &mut wb).unwrap_err().contains("not an axis"));
        let mixed = resolve(&spec(&["data: Expenses.Amount, Expenses.Share"]), &mut wb).unwrap_err();
        assert!(mixed.contains("unit mismatch"), "{mixed}");
    }

    #[test]
    fn histogram_bins() {
        let mut wb = book();
        let d = resolve(&spec(&["type: histogram", "data: Expenses.Amount", "bins: 2"]), &mut wb).unwrap();
        assert_eq!(d.series[0].values, [Some(2.0), Some(1.0)]);
        assert_eq!(d.categories.len(), 2);
    }

    #[test]
    fn sparklines() {
        assert_eq!(sparkline(&[1.0, 2.0, 3.0, 8.0]), "▁▂▃█");
    }
}
