use hardcalc::engine::FOOTER_ROW;
use hardcalc::model::Block;
use hardcalc::value::Value;
use hardcalc::Workbook;

const DOC: &str = "\
## Expenses :: matrix

| Item   | Amount:Euro | Status |
|----------|-------------|--------|
| Rent    | 1200,00     | open  |
| Power    | 85,50       | paid|
| Internet | 39,99       | open  |

```footer Expenses
|         | Item      | Amount                          | Status  |
|---------|-----------|---------------------------------|---------|
| Total   |           | =sum                            | =counta |
| Average |           | =avg                            |         |
| Open    | only open | =sumif(Status, \"open\", Amount) |         |
| Budget  | planned   | 1500,00                         | ok, fine |
```

```types
Euro = decimal(2) unit \"€\"
```

Total: {=Expenses!Amount[Total]}
";

fn workbook(src: &str) -> (hardcalc::Document, Workbook) {
    let doc = hardcalc::parse(src).unwrap();
    let (wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    (doc, wb)
}

#[test]
fn footer_block_belongs_to_the_table() {
    let (doc, _) = workbook(DOC);
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    assert_eq!(t.footer.len(), 4);
    assert_eq!(t.footer[0].label, "Total");
    assert_eq!(t.footer[0].cells, vec!["", "=sum", "=counta"]);
    assert_eq!(t.footer[2].cells[1], "=sumif(Status; \"open\"; Amount)");
    assert_eq!(t.footer[3].cells, vec!["planned", "1500,00", "ok, fine"]);
    assert!(!doc.blocks.iter().any(|b| matches!(b, Block::Code { .. })));
}

#[test]
fn footer_round_trips() {
    let doc = hardcalc::parse(DOC).unwrap();
    let out = hardcalc::write(&doc);
    assert!(out.contains("```footer Expenses\n|  | Item | Amount | Status |"), "{out}");
    assert!(out.contains("| Total |  | =sum | =counta |"), "{out}");
    assert_eq!(hardcalc::write(&hardcalc::parse(&out).unwrap()), out);
}

#[test]
fn footer_values() {
    // Shown with the decimal comma, as with a German locale.
    hardcalc::locale::set_decimal_comma_for_this_thread(true);
    let (_, mut wb) = workbook(DOC);
    let f = |k: usize, c: usize| (0, FOOTER_ROW + k, c);
    assert_eq!(wb.display(f(0, 1)), "1325,49");
    assert_eq!(wb.display(f(0, 2)), "3", "counts show no decimals");
    assert_eq!(wb.display(f(1, 1)), "441,83");
    assert_eq!(wb.display(f(2, 1)), "1239,99");
    assert_eq!(wb.value(f(2, 0)), Value::Text("only open".into()));
    assert_eq!(wb.value(f(1, 0)), Value::Null);
    // Values and texts as in the data: a number in the column's unit, text in a typed column.
    assert_eq!(wb.display(f(3, 1)), "1500,00");
    assert_eq!(wb.eval_formula("Amount[Budget] - Amount[Total]", 0).display(Some(2), true), "174,51 €");
    assert_eq!(wb.value(f(3, 2)), Value::Text("ok, fine".into()));

    // References by name: Table.Footer.Column, and Footer.Column in the own table.
    assert_eq!(wb.eval_formula("Expenses!Amount[Total] * 2", 0).display(Some(2), true), "2650,98 €");
    assert_eq!(wb.eval_formula("Amount[Average]", 0), wb.value(f(1, 1)));
    assert!(wb.eval_formula("Expenses!Amount[Nix]", 0).is_error());

    // Changing data recalculates the footer.
    wb.eval_all();
    wb.set_cell((0, 1, 1), "100");
    assert_eq!(wb.display(f(0, 1)), "1339,99");
    wb.set_footer(0, 1, 1, "=max");
    assert_eq!(wb.display(f(1, 1)), "1200,00");
}

#[test]
fn footer_needs_one_value() {
    let (_, mut wb) = workbook(DOC);
    wb.set_footer(0, 1, 1, "=Amount * 2");
    assert!(wb.value((0, FOOTER_ROW + 1, 1)).is_error());
    wb.set_footer(0, 1, 1, "=@Amount");
    assert!(wb.value((0, FOOTER_ROW + 1, 1)).is_error());
    // Without = it is a text, also when it reads like an aggregate.
    wb.set_footer(0, 1, 1, "sum");
    assert_eq!(wb.value((0, FOOTER_ROW + 1, 1)), Value::Text("sum".into()));
}

#[test]
fn live_value_in_text() {
    // Shown with the decimal comma, as with a German locale.
    hardcalc::locale::set_decimal_comma_for_this_thread(true);
    let (doc, mut wb) = workbook(DOC);
    let out = hardcalc::write(&wb.render(&doc));
    assert!(out.contains("Total: 1325,49 €"), "{out}");
}

#[test]
fn stray_footer_block_is_reported() {
    let src = "Text\n\n```footer\n|       | X    |\n|-------|------|\n| Total | =sum |\n```\n";
    let doc = hardcalc::parse(src).unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    assert!(diags.iter().any(|d| d.message.contains("footer") || d.location == "footer"), "{diags:?}");
    assert_eq!(hardcalc::write(&doc), src, "kept as it is");

    let bad = DOC.replace("| Status  |\n", "| Gibtsnicht |\n");
    let doc = hardcalc::parse(&bad).unwrap();
    let (_, diags) = Workbook::from_document(&doc);
    assert!(diags.iter().any(|d| d.message.contains("unknown column 'Gibtsnicht'")), "{diags:?}");
}

fn footer_of(doc: &hardcalc::Document) -> Vec<Vec<String>> {
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    t.footer.iter().map(|r| r.cells.clone()).collect()
}

#[test]
fn renames_reach_footers() {
    let src = DOC.replace("| only open |", "| =Expenses!Amount[Total] + Amount[Total] |");
    let mut doc = hardcalc::parse(&src).unwrap();
    hardcalc::refs::rename_column(&mut doc, 0, "Amount", "Costs");
    assert_eq!(footer_of(&doc)[2][1], "=sumif(Status; \"open\"; Costs)");
    assert_eq!(footer_of(&doc)[2][0], "=Expenses!Costs[Total] + Costs[Total]");
    assert_eq!(footer_of(&doc)[3][2], "ok, fine", "texts are no formulas");
    assert!(hardcalc::write(&doc).contains("Total: {=Expenses!Costs[Total]}"));

    hardcalc::refs::rename_footer(&mut doc, 0, "Total", "Total");
    assert_eq!(footer_of(&doc)[2][0], "=Expenses!Costs[Total] + Costs[Total]");
    assert!(hardcalc::write(&doc).contains("Total: {=Expenses!Costs[Total]}"));

    hardcalc::refs::rename_table(&mut doc, 0, "Costs 2026");
    assert_eq!(footer_of(&doc)[2][0], "=\"Costs 2026\"!Costs[Total] + Costs[Total]");
    let out = hardcalc::write(&doc);
    assert!(out.contains("```footer Costs 2026\n"), "{out}");
    assert!(out.contains("Total: {=\"Costs 2026\"!Costs[Total]}"), "{out}");
}

#[test]
fn moves_reach_footers() {
    let src = DOC.replace("| =avg                            |", "| =avg(B)                         |");
    let mut doc = hardcalc::parse(&src).unwrap();
    // Swap columns A and B.
    hardcalc::refs::update_document(&mut doc, 0, &hardcalc::refs::Moves { rows: &[0, 1, 2], cols: &[1, 0, 2] });
    assert_eq!(footer_of(&doc)[1][1], "=avg(A)");
}

#[test]
fn eval_prints_footer_values() {
    let (doc, mut wb) = workbook(DOC);
    let out = hardcalc::write(&wb.render(&doc));
    assert!(!out.contains("=sum"), "{out}");
    assert!(out.contains("1325.49") || out.contains("1325,49"), "{out}");
}
