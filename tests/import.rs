use hardcalc::import::import_file;
use hardcalc::model::Block;
use hardcalc::value::Value;
use hardcalc::Workbook;
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook as Xlsx};

/// A file in a directory of its own for each test (tests run in parallel and
/// would otherwise overwrite each other's files).
fn temp(name: &str) -> std::path::PathBuf {
    let test = std::thread::current().name().unwrap_or("main").replace("::", "-");
    let dir = std::env::temp_dir().join(format!("hardcalc-import-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// An xlsx with two sheets, cross-sheet formulas, a date and an untranslatable formula.
fn sample_xlsx() -> std::path::PathBuf {
    let path = temp("mappe.xlsx");
    let mut book = Xlsx::new();
    let date = Format::new().set_num_format("dd.mm.yyyy");
    let s = book.add_worksheet().set_name("Expenses").unwrap();
    s.write_row(0, 0, ["Item", "Amount", "Gross", "Date"]).unwrap();
    for (i, (p, b)) in [("Rent", 1200.0), ("Power", 85.5), ("Internet", 39.99)].into_iter().enumerate() {
        let r = i as u32 + 1;
        s.write(r, 0, p).unwrap();
        s.write(r, 1, b).unwrap();
        s.write_formula(r, 2, Formula::new(format!("=B{}*1.19", r + 1)).set_result(format!("{}", b * 1.19))).unwrap();
        s.write_datetime_with_format(r, 3, ExcelDateTime::from_ymd(2026, 10, i as u8 + 1).unwrap(), &date).unwrap();
    }
    let k = book.add_worksheet().set_name("Metrics").unwrap();
    k.write_row(0, 0, ["Metric", "Value"]).unwrap();
    k.write(1, 0, "Total").unwrap();
    k.write_formula(1, 1, Formula::new("=SUM(Expenses!B2:B4)").set_result("1325.49")).unwrap();
    k.write(2, 0, "Suche").unwrap();
    k.write_formula(2, 1, Formula::new("=VLOOKUP(\"Power\",Expenses!A2:B4,2,FALSE)").set_result("85.5")).unwrap();
    k.write(3, 0, "Column").unwrap();
    k.write_formula(3, 1, Formula::new("=MAX(Expenses!B:B)").set_result("1200")).unwrap();
    book.save(&path).unwrap();
    path
}

fn table<'a>(doc: &'a hardcalc::Document, name: &str) -> &'a hardcalc::model::Table {
    doc.blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) if t.name.as_deref() == Some(name) => Some(t),
            _ => None,
        })
        .unwrap()
}

#[test]
fn xlsx_import() {
    let (doc, report) = import_file(&sample_xlsx()).unwrap();
    assert_eq!((report.tables, report.formulas, report.kept), (2, 6, 1), "{report}");

    let a = table(&doc, "Expenses");
    let names: Vec<&str> = a.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Item", "Amount", "Gross", "Date"]);
    assert_eq!(a.rows[1], ["Power", "85.5", "=B2*1.19", "2026-10-02"]);

    let k = table(&doc, "Metrics");
    assert_eq!(k.rows[0][1], "=sum(Expenses!B1:B3)");
    assert_eq!(k.rows[1][1], "85.5", "VLOOKUP keeps its value");
    assert_eq!(k.rows[2][1], "=max(Expenses!B)");
    let comments = doc.blocks.iter().find_map(|b| match b {
        Block::Layer(l) if l.name == "comments" => Some(l.clone()),
        _ => None,
    });
    let text = format!("{:?}", comments.unwrap().rules);
    assert!(text.contains("B2") && text.contains("original formula: =VLOOKUP(\\\"Power\\\",Expenses!A2:B4,2,FALSE)"), "{text}");

    // The imported document computes the same results.
    let (mut wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    let ks = wb.sheet_index("Metrics").unwrap();
    assert!(matches!(wb.value((ks, 0, 1)), Value::Num(n, _) if (n - 1325.49).abs() < 1e-9));
    assert_eq!(wb.value((ks, 2, 1)), Value::num(1200.0));
    assert!(matches!(wb.value((0, 1, 3)), Value::Date(_)));

    // And it survives writing and reading as hardcalc.
    let again = hardcalc::parse(&hardcalc::write(&doc)).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn ods_import() {
    let (doc, report) = import_file(std::path::Path::new("tests/fixtures/costs.ods")).unwrap();
    assert_eq!((report.tables, report.formulas, report.kept), (1, 2, 0), "{report}");
    let t = table(&doc, "Costs");
    assert_eq!(t.rows[0], ["Rent", "1200.5", "2026-10-01"]);
    assert_eq!(t.rows[2][1], "=sum(B1:B2)");
    assert_eq!(t.rows[2][2], "=today()", "TODAY() has an equivalent now");
}

#[test]
fn unreadable_files_report_errors() {
    let path = temp("kaputt.xlsx");
    std::fs::write(&path, b"not a zip").unwrap();
    assert!(import_file(&path).is_err());
}

#[test]
fn open_and_append_in_the_ui() {
    use hardcalc::tui::app::App;
    let xlsx = sample_xlsx();
    // Opening a spreadsheet imports it into an unsaved document next to it.
    let a = App::open(&xlsx).unwrap();
    assert!(a.dirty);
    assert!(a.path.to_string_lossy().ends_with(".hcmd"));
    assert_eq!(a.wb.sheets.len(), 2);

    // :import adds the sheets to an open document; clashing names are renamed,
    // also inside the translated formulas.
    let mut b = App::new("x.hcmd".into(), hardcalc::parse("## Expenses :: matrix\n\n| a |\n|---|\n| 1 |\n").unwrap());
    b.command(&format!("import {}", xlsx.display()));
    let names: Vec<String> = b.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
    assert_eq!(names, ["Expenses", "Expenses_", "Metrics"]);
    let k = b.wb.sheet_index("Metrics").unwrap();
    assert_eq!(b.wb.sheets[k].raw[0][1], "=sum(Expenses_!B1:B3)");
    assert_eq!(b.view.sheet, 1, "shows the first imported table");
}

#[test]
fn csv_from_german_excel() {
    // Windows-1252, `;`, decimal comma with thousands dots, a quoted field.
    let path = temp("account.csv");
    let mut bytes = b"Date;Recipient;Amount\r\n".to_vec();
    bytes.extend(b"01.10.2026;\"Caf\xE9; Bar\";1.200,50\r\n");
    bytes.extend(b"02.10.2026;Utilities;-85,50\r\n");
    std::fs::write(&path, bytes).unwrap();

    let (doc, report) = import_file(&path).unwrap();
    assert_eq!((report.tables, report.cells), (1, 6));
    let t = table(&doc, "account");
    let names: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Date", "Recipient", "Amount"]);
    assert_eq!(t.rows[0], ["01.10.2026", "Café; Bar", "1.200,50"]);

    let (mut wb, _) = Workbook::from_document(&doc);
    assert_eq!(wb.eval_formula("sum(Amount)", 0), Value::num(1115.0));
    assert!(matches!(wb.value((0, 0, 0)), Value::Date(_)));
}

#[test]
fn csv_export_one_file_per_table_and_back() {
    let doc = hardcalc::parse(include_str!("../examples/budget.hcmd")).unwrap();
    let out = temp("budget.csv");
    let report = hardcalc::export::export_file(&doc, &out).unwrap();
    assert_eq!(report.files.len(), 4, "{report}");
    let ausgaben = temp("budget-Expenses.csv");
    let text = std::fs::read_to_string(&ausgaben).unwrap();
    // Decimal point → `,`; values, not formulas; BOM for spreadsheets.
    assert!(text.starts_with("\u{feff}Item,Amount,Status,Share,Gross\r\nRent,1200.00,paid,0.905,1200.00\r\n"), "{text}");

    // Back in: the same numbers.
    let (back, _) = import_file(&ausgaben).unwrap();
    let (mut wb, _) = Workbook::from_document(&back);
    assert!(matches!(wb.eval_formula("sum(Gross)", 0), Value::Num(n, _) if (n - 1349.34).abs() < 1e-9));

    // From the UI, :export writes just the current table.
    let mut a = hardcalc::tui::app::App::new("b.hcmd".into(), doc);
    a.switch_sheet(2);
    let one = temp("kennzahlen.tsv");
    a.command(&format!("export {}", one.display()));
    let tsv = std::fs::read_to_string(&one).unwrap();
    assert!(tsv.contains("Metric\tValue\r\n") && tsv.contains("Rent\t1200 €"), "{tsv}");
}
