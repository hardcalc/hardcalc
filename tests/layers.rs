use hardcalc::engine::{in_layer, FOOTER_ROW};
use hardcalc::model::{Block, LayerKind};
use hardcalc::value::Value;
use hardcalc::Workbook;

const DOC: &str = "\
## Account :: matrix [January]

| Item | Zugang | Stand        | Doppelt |
|--------|--------|--------------|---------|
| A      | 100    | =C1[-1] + B1 | =B1 * 2 |
| B      | 10     | =C2[-1] + B2 | =B2 * 2 |

```footer Account
|       | Zugang |
|-------|--------|
| Total | =sum   |
```

```slice February
| Item | Zugang |
|--------|--------|
|        | 50     |
|        | 20     |
```

```slice March
| Zugang |
|--------|
| 20     |
```

## Plan :: matrix

| Item | Value |
|--------|------|
| A      | 100  |
| B      | 10   |

```scenario Optimistisch
| Value |
|------|
| 200  |
```

## Metrics :: matrix

| Name  | Value             |
|-------|------------------|
| Basis | =Account!C1        |
| March  | =Account!C1.March   |
";

fn workbook() -> (hardcalc::Document, Workbook) {
    let doc = hardcalc::parse(DOC).unwrap();
    let (wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    (doc, wb)
}

fn num(v: Value) -> f64 {
    match v {
        Value::Num(n, _) => n,
        Value::Null => 0.0,
        v => panic!("not a number: {v:?}"),
    }
}

#[test]
fn file_format() {
    let (doc, _) = workbook();
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    assert_eq!(t.layer_name.as_deref(), Some("January"));
    assert_eq!(t.layers.len(), 2);
    assert_eq!((t.layers[0].name.as_str(), t.layers[0].kind), ("February", LayerKind::Slice));
    assert_eq!(t.layers[1].rows, vec![vec!["", "20", "", ""]]);
    assert_eq!(t.footer.len(), 1);
    let out = hardcalc::write(&doc);
    assert!(out.contains("## Account :: matrix [January]\n"), "{out}");
    assert!(out.contains("```slice March\n| Item | Zugang | Stand | Doppelt |\n"), "{out}");
    assert!(out.contains("```scenario Optimistisch\n"), "{out}");
    assert_eq!(hardcalc::write(&hardcalc::parse(&out).unwrap()), out);
}

#[test]
fn values_through_the_stack() {
    let (_, mut wb) = workbook();
    let v = |wb: &mut Workbook, r: usize, c: usize, z: usize| num(wb.value((0, r, in_layer(c, z))));
    // Formulas of the table work in every layer; [-1] is the layer before.
    assert_eq!([v(&mut wb, 0, 2, 0), v(&mut wb, 0, 2, 1), v(&mut wb, 0, 2, 2)], [100.0, 150.0, 170.0]);
    assert_eq!([v(&mut wb, 1, 2, 0), v(&mut wb, 1, 2, 1), v(&mut wb, 1, 2, 2)], [10.0, 30.0, 30.0]);
    assert_eq!(v(&mut wb, 0, 3, 1), 100.0);
    // A slice keeps its empty cells empty.
    assert_eq!(wb.value((0, 1, in_layer(1, 2))), Value::Null);
    // The footer in each layer.
    assert_eq!(num(wb.value((0, FOOTER_ROW, in_layer(1, 1)))), 70.0);
}

#[test]
fn references_and_ranges() {
    let (_, mut wb) = workbook();
    let f = |wb: &mut Workbook, src: &str| num(wb.eval_formula(src, 0));
    assert_eq!(f(&mut wb, "sum(C1a:c)"), 420.0);
    assert_eq!(f(&mut wb, "sum(C1a:C1c)"), 420.0);
    assert_eq!(f(&mut wb, "sum(B1:2b)"), 70.0);
    assert_eq!(f(&mut wb, "sum(B1a:B2c)"), 200.0);
    assert_eq!(f(&mut wb, "sum(B1:2)"), 110.0);
    assert_eq!(f(&mut wb, "B1b + B1.March + B1.January"), 170.0);
    assert_eq!(f(&mut wb, "sum(Zugang.February)"), 70.0);
    assert_eq!(f(&mut wb, "Account!B1.March + Account!B2b"), 40.0);
    assert_eq!(f(&mut wb, "Zugang[Total].February"), 70.0);
    assert_eq!(f(&mut wb, "offset(B1; 0; 0; 1) + offset(B1; 1; 1; 0)"), 60.0);
    assert!(wb.eval_formula("B1.Mai", 0).is_error());
    assert!(wb.eval_formula("B1d", 0).is_error());
    // Relative to the layer a formula stands in.
    assert_eq!(num(wb.eval_formula_in("B1[-1] + B1", 0, 2)), 70.0);
    assert_eq!(wb.eval_formula_in("B1[+1]", 0, 2), Value::Null, "beyond the stack");
}

#[test]
fn scenarios_take_the_table_values() {
    let (_, mut wb) = workbook();
    assert_eq!(num(wb.value((1, 0, in_layer(1, 1)))), 200.0);
    assert_eq!(num(wb.value((1, 1, in_layer(1, 1)))), 10.0);
    assert_eq!(num(wb.eval_formula("sum(Value.Optimistisch) - sum(Value)", 1)), 100.0);
}

#[test]
fn other_tables_read_layer_a_unless_told() {
    let (_, mut wb) = workbook();
    assert_eq!(num(wb.value((2, 0, 1))), 100.0);
    assert_eq!(num(wb.value((2, 1, 1))), 170.0);
}

#[test]
fn changes_recalculate_the_stack() {
    let (_, mut wb) = workbook();
    wb.eval_all();
    wb.set_layer_cell(0, 1, 0, 1, "60");
    assert_eq!(num(wb.value((0, 0, in_layer(2, 2)))), 180.0);
    wb.set_cell((0, 0, 1), "0");
    assert_eq!(num(wb.value((0, 0, in_layer(2, 2)))), 80.0);
    assert_eq!(num(wb.value((2, 1, 1))), 80.0);
    wb.set_layer_cell(0, 1, 0, 1, "");
    assert_eq!(num(wb.value((0, 0, in_layer(2, 2)))), 20.0, "empty again: no value in February");
}

#[test]
fn layer_names() {
    assert!(hardcalc::parse::valid_layer_name("February"));
    assert!(hardcalc::parse::valid_layer_name("Q1 2026"));
    for bad in ["b", "B2", "a.b", "x:y", "", "12"] {
        assert!(!hardcalc::parse::valid_layer_name(bad), "{bad}");
    }
    // A data layer that does not fit is kept and reported.
    let bad = DOC.replace("```slice March\n| Zugang |", "```slice March\n| Gibtsnicht |");
    let doc = hardcalc::parse(&bad).unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    assert!(diags.iter().any(|d| d.message.contains("block ignored")), "{diags:?}");
    assert!(hardcalc::write(&doc).contains("| Gibtsnicht |"));
}

#[test]
fn other_tables_are_read_in_layer_a_unless_named() {
    let (_, mut wb) = workbook();
    // In layer b of Account: Plan has a layer b too, but it is another stack.
    assert_eq!(num(wb.eval_formula_in("Plan!B1", 0, 1)), 100.0);
    assert_eq!(num(wb.eval_formula_in("sum(Plan!Value)", 0, 1)), 110.0);
    assert_eq!(num(wb.eval_formula_in("Plan!B1.Optimistisch + Plan!B1b", 0, 1)), 400.0);
    assert_eq!(num(wb.eval_formula_in("offset(Plan!B1; 1; 0)", 0, 1)), 10.0);
    // In its own table a reference keeps the formula's layer.
    assert_eq!(num(wb.eval_formula_in("Account!B1", 0, 1)), 50.0);
}

#[test]
fn range_targets_survive_parse_and_write() {
    let src = "## T :: matrix\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n```layer format\nA1:A2: bg green\n```\n\n```layer comments\nB1: Note: x\n```\n";
    let doc = hardcalc::parse(src).unwrap();
    let out = hardcalc::write(&doc);
    assert!(out.contains("A1:A2: bg green"), "{out}");
    assert!(out.contains("B1: Note: x"), "{out}");
    let (wb, _) = Workbook::from_document(&doc);
    let att = hardcalc::layers::attach(&doc, &wb);
    let f = hardcalc::layers::Formats::new(&att, "format");
    assert!(f.errors.is_empty(), "{:?}", f.errors);
}

#[test]
fn layer_errors_know_their_line() {
    let src = "## T :: matrix\n\n| A |\n|---|\n| 1 |\n\n```layer format\nA1: bold\nA1: bogus\n```\n";
    let doc = hardcalc::parse(src).unwrap();
    let (wb, _) = Workbook::from_document(&doc);
    let att = hardcalc::layers::attach(&doc, &wb);
    let f = hardcalc::layers::Formats::new(&att, "format");
    assert_eq!(f.errors.len(), 1);
    assert_eq!(f.errors[0].line, 9);
}

/// The function table of the LibreOffice extension must list the same functions
/// as `BUILTINS`: same Calc name and argument kinds.
#[test]
fn extension_knows_every_builtin() {
    let py = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/libreoffice/extension/pythonpath/hardcalc_lo/builtins.py")).unwrap();
    let table = py.split("BUILTINS = {").nth(1).unwrap().split("\n}").next().unwrap();
    let mut python: Vec<(String, String, String)> = table
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (name, rest) = l.strip_prefix('"')?.split_once("\": (\"")?;
            let (excel, rest) = rest.split_once("\", \"")?;
            Some((name.to_string(), excel.to_string(), rest.split('"').next()?.to_string()))
        })
        .collect();
    let mut rust: Vec<(String, String, String)> = hardcalc::builtins::BUILTINS
        .iter()
        .filter_map(|b| b.excel.map(|e| (b.name.to_string(), e.to_string(), b.args.to_string())))
        .collect();
    python.sort();
    rust.sort();
    assert_eq!(python, rust, "hardcalc_lo/builtins.py and src/builtins.rs differ");
}
