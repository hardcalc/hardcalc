use hardcalc::units::Unit;
use hardcalc::value::Value;
use hardcalc::{parse, Workbook};

const BUDGET: &str = include_str!("../examples/budget.hcmd");

fn book(src: &str) -> Workbook {
    let doc = parse(src).unwrap();
    Workbook::from_document(&doc).0
}

fn euro(n: f64) -> Value {
    Value::Num(n, Unit::symbol("€"))
}

#[test]
fn example_has_no_errors() {
    let doc = parse(BUDGET).unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn example_values() {
    let mut wb = book(BUDGET);
    let ausgaben = wb.sheet_index("Expenses").unwrap();
    let kennzahlen = wb.sheet_index("Metrics").unwrap();

    // Shares are rounded to decimal(3) and add up to ~1.
    let shares: f64 = (0..3)
        .map(|r| match wb.value((ausgaben, r, 3)) {
            Value::Num(n, u) if u.is_none() => n,
            v => panic!("{v:?}"),
        })
        .sum();
    assert!((shares - 1.0).abs() < 0.002);

    // decimal(2) rounding of a vector formula with implicit intersection.
    assert_eq!(wb.value((ausgaben, 1, 4)), euro(101.75));
    assert_eq!(wb.value((kennzahlen, 0, 1)), euro(1200.0 + 101.75 + 47.59));
    assert_eq!(wb.value((kennzahlen, 1, 1)), euro(1200.0));
    assert_eq!(wb.value((kennzahlen, 2, 1)), euro(255.0));
    // 12 * 5 m/s via row label `Speed`.
    assert_eq!(wb.value((kennzahlen, 4, 1)), Value::Num(60.0, Unit::parse("m/s").unwrap()));
    assert!(!wb.decimal_comma, "shown with the decimal point unless the locale says otherwise");
}

#[test]
fn incremental_recalculation() {
    let mut wb = book(BUDGET);
    let a = wb.sheet_index("Expenses").unwrap();
    let k = wb.sheet_index("Metrics").unwrap();
    assert_eq!(wb.value((k, 1, 1)), euro(1200.0));
    let _ = wb.value((k, 4, 1));
    let _ = wb.value((k, 0, 1));

    wb.set_cell((a, 0, 1), "1300,00");
    // Dependents of the changed cell were dropped, unrelated results stay cached.
    assert!(!wb.is_cached((k, 1, 1)));
    assert!(!wb.is_cached((k, 0, 1)));
    assert!(!wb.is_cached((a, 0, 4)));
    assert!(wb.is_cached((k, 4, 1)));
    assert_eq!(wb.value((k, 1, 1)), euro(1300.0));
    assert_eq!(wb.value((k, 0, 1)), euro(1300.0 + 101.75 + 47.59));
}

#[test]
fn unit_errors() {
    let mut wb = book(
        "## T :: matrix\n\n| a | b:decimal(2) unit \"€\" |\n|---|---|\n| 5 m | 3 |\n| =A1 + B1 | =A1 |\n",
    );
    assert!(matches!(wb.value((0, 1, 0)), Value::Error(e) if e.contains("unit mismatch")));
    assert!(matches!(wb.value((0, 1, 1)), Value::Error(e) if e.contains("expected unit €")));
}

#[test]
fn cycles_are_detected() {
    let mut wb = book("| a | b |\n|---|---|\n| =B1 | =A1 |\n");
    assert!(matches!(wb.value((0, 0, 0)), Value::Error(_)));
}

#[test]
fn type_diagnostics() {
    let doc = parse("```types\nS = enum(x, y)\n```\n\n| s:S | n:int | z:Nope |\n|---|---|---|\n| q | 1.5 | 1 |\n").unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    let msgs: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(diags.len(), 3, "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("unknown type 'Nope'")));
    assert!(msgs.iter().any(|m| m.starts_with("A1:")));
    assert!(msgs.iter().any(|m| m.starts_with("B1:")));
}

#[test]
fn functions_and_dates() {
    let mut wb = book(
        "| d | x |\n|---|---|\n| 2026-10-08 | 4 |\n| 2026-12-24 | =if(A2 - A1 > 30{d}; sqrt(B1); 0) |\n| =A2 - A1 | =round(mean(B1:B2) / 3; 2) |\n",
    );
    assert_eq!(wb.value((0, 1, 1)), Value::num(2.0));
    assert_eq!(wb.value((0, 2, 0)), Value::Num(77.0, Unit::symbol("d")));
    assert_eq!(wb.value((0, 2, 1)), Value::num(1.0));
}

#[test]
fn references_to_other_tables() {
    let mut wb = book(BUDGET);
    let k = wb.sheet_index("Metrics").unwrap();
    assert_eq!(wb.eval_formula("Expenses!B2", k), euro(85.5));
    assert_eq!(wb.eval_formula("sum(Expenses!B1:B2)", k), euro(1285.5));
    assert_eq!(wb.eval_formula("sum(Expenses!B)", k), euro(1325.49));
    assert!(wb.eval_formula("Nope!B2", k).is_error());
}

#[test]
fn user_functions() {
    let src = format!(
        "{BUDGET}\n```functions\ngross(x) = x * 1.19   # with VAT\nshare(part; total) = part / total\n\
         fak(n) = if(n <= 1; 1; n * fak(n - 1))\nendlos(n) = endlos(n + 1)\nposten(Amount) = Amount * 2\n```\n"
    );
    let mut wb = book(&src);
    let a = wb.sheet_index("Expenses").unwrap();
    assert!(matches!(wb.eval_formula("gross(B2)", a), Value::Num(n, _) if (n - 101.745).abs() < 1e-9));
    assert!(matches!(wb.eval_formula("share(B1; sum(B))", a), Value::Num(n, _) if (n - 1200.0 / 1325.49).abs() < 1e-12));
    // Columns as arguments: element-wise, units kept.
    assert!(matches!(wb.eval_formula("sum(gross(Amount))", a), Value::Num(n, u) if (n - 1577.3331).abs() < 1e-9 && u == Unit::symbol("€")));
    assert_eq!(wb.eval_formula("fak(5)", a), Value::num(120.0));
    assert!(matches!(wb.eval_formula("endlos(1)", a), Value::Error(e) if e.contains("nested too deeply")));
    assert!(matches!(wb.eval_formula("gross(1; 2)", a), Value::Error(e) if e.contains("expects 1 argument")));
    // A parameter shadows a column of the same name.
    assert_eq!(wb.eval_formula("posten(3)", a), Value::num(6.0));
}

#[test]
fn function_errors_are_reported() {
    let doc = parse("```functions\nsum(x) = x\nkaputt\nf(x) = x +\n```\n").unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    assert_eq!(diags.len(), 3, "{diags:?}");
}

#[test]
fn at_references_take_this_row() {
    let src = "## Preise :: matrix[*, 3]\n\n| Article | Price | Quantity |\n|---|---|---|\n| A | 2 | 3 |\n| B | 5 | 4 |\n\n\
               ## Invoice :: matrix[*, 4]\n\n| Article | Net | Share | Text |\n|---|---|---|---|\n\
               | A | =Preise.Price * Preise.Quantity | =@Net / sum(Net) | =concat(@Article, \": \", [@Net]) |\n\
               | B | =Preise[@Price] * @Preise.Quantity | =round(@Net / sum(Net), 2) | =upper(@Article) |\n";
    let doc = hardcalc::parse(src).unwrap();
    let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
    let r = wb.sheet_index("Invoice").unwrap();
    let v = |wb: &mut hardcalc::Workbook, row, col| wb.value((r, row, col)).display(None, false);
    assert_eq!(v(&mut wb, 0, 1), "6");
    assert_eq!(v(&mut wb, 1, 1), "20", "Preise[@Price] and @Preise.Quantity: the same row there");
    assert_eq!(v(&mut wb, 0, 2), "0.2307692308");
    assert_eq!(v(&mut wb, 1, 2), "0.77");
    assert_eq!(v(&mut wb, 0, 3), "A: 6", "@ inside a function that would otherwise take the column");
    assert_eq!(v(&mut wb, 1, 3), "B");
}

mod block1_functions {
    use hardcalc::value::Value;
    use hardcalc::Workbook;

    const DOC: &str = "## T :: matrix\n\n| Name | Start | Grp | Amount |\n|------|-------|-----|--------|\n| a | 2024-01-31 | x | 10 |\n| b |  | y | 20 |\n| c | 2020-02-29 | x | 5 |\n";

    fn eval(src: &str) -> Value {
        let doc = hardcalc::parse(DOC).unwrap();
        let (mut wb, diags) = Workbook::from_document(&doc);
        assert!(diags.is_empty(), "{diags:?}");
        wb.eval_formula(src, 0)
    }

    fn text(src: &str) -> String {
        eval(src).display(None, false)
    }

    #[test]
    fn information_functions() {
        assert_eq!(text("isblank(B2)"), "true");
        assert_eq!(text("isblank(B1)"), "false");
        assert_eq!(text("isnumber(5)"), "true");
        assert_eq!(text("isnumber(B1)"), "true");
        assert_eq!(text("isnumber(\"5\")"), "false");
        assert_eq!(text("istext(\"a\")"), "true");
        assert_eq!(text("iserror(1/0)"), "true");
        assert_eq!(text("iserror(1)"), "false");
        assert_eq!(text("isblank(B)"), "[false; true; false]");
    }

    #[test]
    fn month_arithmetic() {
        assert_eq!(text("edate(B1; 1)"), "2024-02-29");
        assert_eq!(text("edate(date(2024;1;31); -2)"), "2023-11-30");
        assert_eq!(text("edate(date(2023;12;15); 1)"), "2024-01-15");
        assert_eq!(text("eomonth(B3; 12)"), "2021-02-28");
        assert_eq!(text("eomonth(date(2024;2;10); 0)"), "2024-02-29");
        assert!(eval("edate(B2; 1)").is_error(), "an empty cell is no date");
    }

    #[test]
    fn datedif_as_calc_computes_it() {
        let d = |a: &str, b: &str, u: &str| text(&format!("datedif({a}; {b}; \"{u}\")"));
        assert_eq!(d("date(2020;2;29)", "date(2024;2;28)", "Y"), "3");
        assert_eq!(d("date(2020;1;15)", "date(2024;3;10)", "M"), "49");
        assert_eq!(d("date(2020;1;15)", "date(2024;3;10)", "YM"), "1");
        assert_eq!(d("date(2020;1;15)", "date(2020;1;25)", "d"), "10");
        // The roll-over cases from Calc's source.
        assert_eq!(d("date(2012;1;28)", "date(2012;3;1)", "MD"), "2");
        assert_eq!(d("date(2011;1;29)", "date(2011;3;1)", "MD"), "0");
        assert_eq!(d("date(1988;6;22)", "date(2012;5;11)", "YD"), "324");
        assert!(eval("datedif(date(2020;1;15); date(2020;1;10); \"D\")").is_error());
        assert!(eval("datedif(date(2020;1;15); date(2021;1;10); \"X\")").is_error());
    }

    #[test]
    fn ifs_and_switch_take_one_branch() {
        assert_eq!(text("ifs(1>2; \"a\"; 2>1; \"b\")"), "b");
        assert_eq!(text("ifs(true; 1; 1/0; 2)"), "1");
        assert!(eval("ifs(1>2; 1)").is_error());
        assert_eq!(text("switch(3; 1; \"one\"; 3; \"three\"; \"other\")"), "three");
        assert_eq!(text("switch(9; 1; \"one\"; \"other\")"), "other");
        assert_eq!(text("switch(\"X\"; \"x\"; 1; 2)"), "1", "text without regard to case");
        assert!(eval("switch(9; 1; 2)").is_error());
        assert_eq!(text("switch(1; 1; 5; 1/0)"), "5");
    }

    #[test]
    fn conditional_aggregates() {
        assert_eq!(text("averageifs(Amount; Grp; \"x\")"), "7.5");
        assert_eq!(text("minifs(Amount; Grp; \"x\")"), "5");
        assert_eq!(text("maxifs(Amount; Grp; \"x\")"), "10");
        assert_eq!(text("maxifs(Amount; Grp; \"z\")"), "0", "no match gives 0, like Calc");
        assert_eq!(text("sumifs(Amount; Grp; \"x\")"), "15");
    }
}

#[test]
fn whole_column_ranges_sum_columns() {
    let mut wb = book("## T :: matrix\n\n| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n");
    assert_eq!(wb.eval_formula("sum(B:B)", 0), Value::num(7.0));
    assert_eq!(wb.eval_formula("sum(B:C)", 0), Value::num(16.0));
    assert_eq!(wb.eval_formula("sum(C:A)", 0), Value::num(21.0));
    assert!(wb.eval_formula("sum(B:2)", 0).is_error());
}

#[test]
fn subtotal_counts_the_rows_the_filter_shows() {
    let src = "\
## Journal :: matrix

| Item | Amount | Status |
|---|--:|---|
| Rent | 1200 | paid |
| Power | 85.5 | open |
| Phone | 20 | open |

```footer
| | Item | Amount |
|---|---|---|
| Total | =subtotal(counta) | =sum |
| Shown | | =subtotal(sum) |
```

```layer filter
Status: = open
```

## Summary :: matrix

| What | Value |
|---|---|
| open | =subtotal(sum; Journal!Amount) |
| mean | =subtotal(avg; Journal!B) |
| part | =subtotal(sum; Journal!B1:B2) |
| own | =subtotal(max; Value) |
";
    let doc = parse(src).unwrap();
    let (mut wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    let footer = hardcalc::engine::FOOTER_ROW;
    assert_eq!(wb.value((0, footer, 1)), Value::num(1305.5), "sum counts every row");
    assert_eq!(wb.value((0, footer + 1, 1)), Value::num(105.5));
    assert_eq!(wb.value((0, footer, 0)), Value::num(2.0));
    assert_eq!(wb.value((1, 0, 1)), Value::num(105.5));
    assert_eq!(wb.value((1, 1, 1)), Value::num(52.75));
    assert!(wb.value((1, 2, 1)).is_error(), "a part of a column has no rows to match");
    // A row that starts to meet the filter counts at once.
    wb.set_cell((0, 0, 2), "open");
    assert_eq!(wb.value((0, footer + 1, 1)), Value::num(1305.5));
    assert_eq!(wb.value((1, 0, 1)), Value::num(1305.5));
    wb.set_cell((0, 0, 2), "paid");
    // With the filter switched off (the view shows all rows) all rows count.
    wb.set_filter_off(0, true);
    assert_eq!(wb.value((0, footer + 1, 1)), Value::num(1305.5));
    wb.set_filter_off(0, false);
    assert_eq!(wb.value((0, footer + 1, 1)), Value::num(105.5));
    // Excel: SUBTOTAL over the column.
    let names = vec!["Journal".to_string(), "Summary".to_string()];
    assert_eq!(hardcalc::export::formula_to_excel(&wb, &names, 1, 0, "subtotal(sum; Journal!Amount)").unwrap(), "=SUBTOTAL(109,'Journal'!$B$2:$B$4)");
}
