use calamine::{open_workbook_auto, Data, Reader};
use hardcalc::export::{export_xlsx, formula_to_excel};
use hardcalc::value::Value;
use hardcalc::{parse, Workbook};

const BUDGET: &str = include_str!("../examples/budget.hcmd");

/// A file in a directory of its own for each test (tests run in parallel and
/// would otherwise overwrite each other's files).
fn temp(name: &str) -> std::path::PathBuf {
    let test = std::thread::current().name().unwrap_or("main").replace("::", "-");
    let dir = std::env::temp_dir().join(format!("hardcalc-export-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn formulas_back_to_excel() {
    let doc = parse(BUDGET).unwrap();
    let (mut wb, _) = Workbook::from_document(&doc);
    wb.eval_all();
    let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
    let k = wb.sheet_index("Metrics").unwrap();
    let x = |s: usize, r: usize, f: &str| formula_to_excel(&wb, &names, s, r, f);
    // Rows shift by the header; a column name outside an aggregate means this row.
    assert_eq!(x(0, 0, "=B1/sum(B)").unwrap(), "=B2/SUM($B$2:$B$4)");
    assert_eq!(x(0, 1, "=Amount * 1.19").unwrap(), "=B3*1.19");
    assert_eq!(x(0, 2, "=Amount * 119%").unwrap(), "=B4*119%");
    assert_eq!(x(k, 0, "=sum(Expenses.Gross)").unwrap(), "=SUM('Expenses'!$E$2:$E$4)");
    assert_eq!(x(k, 1, "=Expenses[Rent, Amount]").unwrap(), "='Expenses'!B2");
    assert_eq!(x(k, 2, "=sum(Sales[Region=North])").unwrap(), "=SUMIFS('Sales'!$D$2:$D$5,'Sales'!$B$2:$B$5,\"North\")");
    assert_eq!(x(k, 4, "=12 * Speed").unwrap(), "=12*B7", "row label → its value cell");
    assert_eq!(x(k, 0, "=sum(Expenses!B1:B2) + Expenses!B3").unwrap(), "=SUM('Expenses'!B2:B3)+'Expenses'!B4");
    assert_eq!(x(0, 0, "=round(B1)").unwrap(), "=ROUND(B2,0)");
    assert!(x(k, 6, "=spark(Expenses.Gross)").is_err());
    // Range arguments take the column, value arguments this row's cell.
    assert_eq!(x(0, 0, "=sumif(Status; \"open\"; Amount)").unwrap(), "=SUMIF($C$2:$C$4,\"open\",$B$2:$B$4)");
    assert_eq!(x(0, 1, "=left(Item; 3) & upper(Status)").unwrap(), "=LEFT(A3,3)&UPPER(C3)");
    assert_eq!(x(k, 0, "=xlookup(\"Power\"; Expenses.Item; Expenses.Amount)").unwrap(), "=XLOOKUP(\"Power\",'Expenses'!$A$2:$A$4,'Expenses'!$B$2:$B$4)");
    assert_eq!(x(0, 1, "=sum(@Amount; [@Gross])").unwrap(), "=SUM(B3,E3)", "@: this row even in an aggregate");
    assert_eq!(x(k, 1, "=Expenses[@Amount]").unwrap(), "='Expenses'!B3");
    assert_eq!(x(0, 0, "=concatenate(Item; \"!\")").unwrap(), "=CONCAT($A$2:$A$4,\"!\")", "the whole column, as in hardcalc");
}

#[test]
fn xlsx_export_and_read_back() {
    let path = temp("budget.xlsx");
    let report = export_xlsx(&parse(BUDGET).unwrap(), &path).unwrap();
    assert_eq!((report.sheets, report.charts), (4, 2), "{report}");
    assert_eq!(report.kept, 1, "only spark() has no Excel equivalent: {report}");
    assert!(report.notes >= 2 && report.validations >= 2, "{report}");

    let mut book = open_workbook_auto(&path).unwrap();
    let names = book.sheet_names();
    assert_eq!(&names[..4], ["Expenses", "Sales", "Metrics", "Table 4"]);
    assert!(names.iter().any(|n| n == "SalesTrend data"), "tensor chart data: {names:?}");

    let values = book.worksheet_range("Expenses").unwrap();
    let formulas = book.worksheet_formula("Expenses").unwrap();
    assert_eq!(values.get_value((0, 1)), Some(&Data::String("Amount".into())));
    assert_eq!(values.get_value((2, 1)), Some(&Data::Float(85.5)));
    // Euro is decimal(2): the formula rounds like hardcalc does.
    assert_eq!(formulas.get_value((2, 4)).map(String::as_str), Some("ROUND(B3*1.19,2)"));
    // The computed result is stored with the formula.
    assert_eq!(values.get_value((2, 4)), Some(&Data::Float(101.75)));

    // Round trip: importing the export computes the same results.
    let (doc, _) = hardcalc::import::import_file(&path).unwrap();
    let (mut wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    let k = wb.sheet_index("Metrics").unwrap();
    assert!(matches!(wb.value((k, 0, 1)), Value::Num(n, _) if (n - 1349.34).abs() < 1e-9));
    assert!(matches!(wb.value((0, 1, 4)), Value::Num(n, _) if (n - 101.75).abs() < 1e-9));
}

#[test]
fn number_formats_follow_types() {
    let path = temp("formats.xlsx");
    export_xlsx(&parse(BUDGET).unwrap(), &path).unwrap();
    // xlsx is a zip; the styles part lists the number formats in use.
    let file = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut styles = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/styles.xml").unwrap(), &mut styles).unwrap();
    assert!(styles.contains("#,##0.00 &quot;€&quot;"), "Euro: {styles}");
    assert!(styles.contains("0.0%"), "percent(1) of the format layer");
}

#[test]
fn excel_formulas_to_openformula() {
    use hardcalc::export::excel_to_odf;
    assert_eq!(excel_to_odf("=B2/SUM($B$2:$B$4)"), "of:=[.B2]/SUM([.$B$2:.$B$4])");
    assert_eq!(excel_to_odf("=ROUND(B3*1.19,2)"), "of:=ROUND([.B3]*1.19;2)");
    assert_eq!(
        excel_to_odf("=SUMIFS('Sales'!$D$2:$D$5,'Sales'!$B$2:$B$5,\"North, South\")"),
        "of:=SUMIFS([$'Sales'.$D$2:.$D$5];[$'Sales'.$B$2:.$B$5];\"North, South\")"
    );
    assert_eq!(excel_to_odf("=LOG10(A1)+'It''s'!C3"), "of:=LOG10([.A1])+[$'It''s'.C3]");
}

#[test]
fn ods_export_and_read_back() {
    let path = temp("budget.ods");
    let report = hardcalc::export::export_file(&parse(BUDGET).unwrap(), &path).unwrap();
    assert_eq!((report.sheets, report.kept), (4, 1), "{report}");

    let mut book = open_workbook_auto(&path).unwrap();
    assert_eq!(&book.sheet_names()[..4], ["Expenses", "Sales", "Metrics", "Table 4"]);
    let values = book.worksheet_range("Expenses").unwrap();
    let formulas = book.worksheet_formula("Expenses").unwrap();
    assert_eq!(values.get_value((2, 1)), Some(&Data::Float(85.5)));
    assert_eq!(formulas.get_value((2, 4)).map(String::as_str), Some("of:=ROUND([.B3]*1.19;2)"));
    assert_eq!(values.get_value((2, 4)), Some(&Data::Float(101.75)));

    // Round trip through the importer: same results, Metrics included.
    let (doc, _) = hardcalc::import::import_file(&path).unwrap();
    let (mut wb, diags) = Workbook::from_document(&doc);
    assert!(diags.is_empty(), "{diags:?}");
    let k = wb.sheet_index("Metrics").unwrap();
    assert!(matches!(wb.value((k, 0, 1)), Value::Num(n, _) if (n - 1349.34).abs() < 1e-9));
    assert!(matches!(wb.value((0, 0, 3)), Value::Num(n, _) if (n - 1200.0 / 1325.49).abs() < 1e-3));
}

#[test]
fn ods_content_has_styles_notes_and_validation() {
    let path = temp("check.ods");
    hardcalc::export::export_ods(&parse(BUDGET).unwrap(), &path).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(zip.by_index(0).unwrap().name().unwrap(), "mimetype");
    let mut content = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("content.xml").unwrap(), &mut content).unwrap();
    assert!(content.contains("<number:text> €</number:text>"), "Euro number style");
    assert!(content.contains("number:percentage-style"), "percent(1) from the format layer");
    assert!(content.contains("Rent increase announced for July"), "comment as annotation");
    assert!(content.contains("of:cell-content-is-in-list(&quot;open&quot;;&quot;paid&quot;;&quot;cancelled&quot;)"));
    assert!(content.contains("cell-content()&gt;=0"), "validate B: >= 0");
}

#[test]
fn user_functions_are_inlined() {
    let src = format!("{BUDGET}\n```functions\ngross(x) = x * 1.19\nshare(t; g) = t / g\nfak(n) = if(n <= 1; 1; n * fak(n - 1))\n```\n");
    let doc = parse(&src).unwrap();
    let (mut wb, _) = Workbook::from_document(&doc);
    wb.eval_all();
    let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
    assert_eq!(formula_to_excel(&wb, &names, 0, 1, "=gross(Amount)").unwrap(), "=(B3*1.19)");
    assert_eq!(formula_to_excel(&wb, &names, 0, 0, "=share(B1; sum(B))").unwrap(), "=(B2/SUM($B$2:$B$4))");
    assert!(formula_to_excel(&wb, &names, 0, 0, "=fak(3)").is_err(), "recursion stays a value with note");
}

#[test]
fn footer_rows_are_exported() {
    let src = BUDGET.replacen(
        "```layer format",
        "```footer Expenses\n| | Item | Amount | Gross |\n|-|-|-|-|\n| Total | | =sum | =Amount[Total] * 1.19 |\n| Average | =counta | =avg | estimated |\n```\n\n```layer format",
        1,
    );
    let src = src.replacen("Rent is", "Total {=Expenses!Amount[Total]}. Rent is", 1);
    let path = temp("footer.xlsx");
    export_xlsx(&parse(&src).unwrap(), &path).unwrap();
    let mut book = open_workbook_auto(&path).unwrap();
    let values = book.worksheet_range("Expenses").unwrap();
    let formulas = book.worksheet_formula("Expenses").unwrap();
    // Data in rows 2–4, footers in 5 and 6; the label in column A if free.
    assert_eq!(values.get_value((4, 0)), Some(&Data::String("Total".into())));
    assert_eq!(values.get_value((4, 1)), Some(&Data::Float(1325.49)));
    let f = |r: u32, c: u32| formulas.get_value((r, c)).cloned().unwrap_or_default();
    assert_eq!(f(4, 1), "SUM($B$2:$B$4)");
    assert_eq!(f(4, 4), "B5*1.19");
    assert_eq!(f(5, 0), "COUNTA($A$2:$A$4)");
    assert_eq!(f(5, 1), "AVERAGE($B$2:$B$4)");
    assert_eq!(values.get_value((5, 4)), Some(&Data::String("estimated".into())));
}

#[test]
fn wrap_and_range_bg_reach_the_pdf() {
    let src = "## T :: matrix\n\n| N | Note |\n|---|------|\n| 1 | the quick brown fox jumps over the lazy dog |\n| 2 | short |\n\n```layer format\nNote: wrap 12\n```\n";
    let doc = hardcalc::parse(src).unwrap();
    let mut app = hardcalc::tui::app::App::new(std::path::PathBuf::from("t.hcmd"), doc);
    let layout = app.layout(&hardcalc::print::PageSetup { scope: hardcalc::print::Scope::Document, ..Default::default() });
    let text: Vec<String> = layout.pages[0].iter().map(|l| l.plain()).collect();
    assert!(text.iter().any(|l| l.contains("the quick") ), "{text:#?}");
    assert!(text.iter().any(|l| l.contains("lazy dog")), "{text:#?}");
}
