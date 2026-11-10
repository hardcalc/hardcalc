//! Any Markdown opens, and saving never changes what hardcalc does not understand.

use hardcalc::model::Block;
use hardcalc::{parse, write};

fn tables(src: &str) -> Vec<(Option<String>, Vec<Vec<String>>)> {
    parse(src).unwrap().blocks.into_iter().filter_map(|b| if let Block::Table(t) = b { Some((t.name, t.rows)) } else { None }).collect()
}

#[test]
fn texts_that_look_like_numbers_stay_as_they_are() {
    let src = "## T :: matrix\n\n| k | v |\n|---|---|\n| version | 1.2.3 |\n| ip | 192.168.1.1 |\n| phone | 030 1234567 |\n| zip | 01234 |\n| odd | 1.5,2 |\n";
    assert_eq!(write(&parse(src).unwrap()), src);
    let (mut wb, _) = hardcalc::Workbook::from_document(&parse(src).unwrap());
    for r in 0..5 {
        assert!(matches!(wb.value((0, r, 1)), hardcalc::value::Value::Text(_)), "row {r}");
    }
}

#[test]
fn numbers_with_thousands_and_decimal_commas() {
    use hardcalc::value::parse_number;
    for (text, n, comma) in [
        ("1200,50", 1200.5, true),
        ("1.200,50", 1200.5, true),
        ("1,200.50", 1200.5, false),
        ("1.200.000", 1_200_000.0, true),
        ("1,234,567", 1_234_567.0, false),
        ("1 200 000", 1_200_000.0, false),
        ("1'200,5", 1200.5, true),
        ("-1.200,5", -1200.5, true),
        ("1,200", 1.2, true),
        ("1.200", 1.2, false),
        ("+5", 5.0, false),
        ("1e3", 1000.0, false),
    ] {
        assert_eq!(parse_number(text), Some((n, comma)), "{text}");
    }
    for text in ["1.2.3", "192.168.1.1", "030 1234567", "12 34", "1,5,2", "1.5,2,0", "1.2,3.4", "abc"] {
        assert_eq!(parse_number(text), None, "{text}");
    }
}

#[test]
fn any_markdown_opens() {
    // A heading with `::` that names no shape is a heading.
    let t = tables("## std::map overview\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, None);
    let src = "## std::map overview\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    assert_eq!(write(&parse(src).unwrap()), src);
    assert!(hardcalc::parse::notes(src)[0].message.contains("not a table heading"));
    // An unclosed code block runs to the end.
    let doc = parse("# x\n\n```python\nprint(1)\n| a |\n|---|\n").unwrap();
    assert!(matches!(&doc.blocks[1], Block::Code { info, body } if info == "python" && body.len() == 3));
    assert_eq!(hardcalc::parse::notes("```\nx\n").len(), 1);
    // A byte order mark is not part of the first heading.
    let t = tables("\u{feff}## Loads :: matrix\n\n| a |\n|---|\n| 1 |\n");
    assert_eq!(t[0].0.as_deref(), Some("Loads"));
}

#[test]
fn tables_in_code_and_comments_are_no_tables() {
    for src in [
        "~~~\n| a | b |\n|---|---|\n| 1,5 | 2 |\n~~~\n",
        "`````\n```\n| a | b |\n|---|---|\n| 1,5 | 2 |\n```\n`````\n",
        "Text\n\n    | a | b |\n    |---|---|\n    | 1,5 | 2 |\n",
        "<!--\n| a | b |\n|---|---|\n| 1,5 | 2 |\n-->\n",
    ] {
        assert!(tables(src).is_empty(), "{src}");
        let out = write(&parse(src).unwrap());
        assert!(out.contains("| 1,5 | 2 |"), "{out}");
        assert_eq!(write(&parse(&out).unwrap()), out, "fmt is idempotent");
    }
    // A fence closes only with a line of its own kind and length.
    let doc = parse("````\n```hardcalc\nx = 1\n```\n````\n\n| a |\n|---|\n| 1 |\n").unwrap();
    assert!(matches!(&doc.blocks[0], Block::Code { body, .. } if body.len() == 3));
    assert!(matches!(&doc.blocks[1], Block::Table(_)));
}

#[test]
fn column_headers_with_colons() {
    let doc = parse("| Time 12:00 | Rate:decimal(1) special(\"-\": \"n/a\") | Amount:Euro | Addr:record(street: text, zip: text) |\n|---|---|---|---|\n| 1 | 2 | 3 | 4 |\n").unwrap();
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    let cols: Vec<(&str, Option<&str>)> = t.columns.iter().map(|c| (c.name.as_str(), c.ty.as_deref())).collect();
    assert_eq!(
        cols,
        [
            ("Time 12:00", None),
            ("Rate", Some("decimal(1) special(\"-\": \"n/a\")")),
            ("Amount", Some("Euro")),
            ("Addr", Some("record(street: text, zip: text)")),
        ]
    );
    assert_eq!(write(&parse(&write(&doc)).unwrap()), write(&doc));
}

#[test]
fn lower_case_names_of_slices() {
    let src = "## Account :: matrix [january]\n\n| Item | In |\n|---|---|\n| S | 1 |\n\n```slice plan\n| In |\n|---|\n| 5 |\n```\n";
    let doc = parse(src).unwrap();
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    assert_eq!(t.layer_name.as_deref(), Some("january"));
    assert_eq!(t.layers[0].name, "plan");
    // One or two lower-case letters are the letters of levels.
    assert!(!hardcalc::parse::valid_layer_name("b") && !hardcalc::parse::valid_layer_name("ab"));
    assert!(hardcalc::parse::valid_layer_name("abc"));
}

#[test]
fn notes_on_what_is_dropped_or_double() {
    let src = "## T :: matrix\n\n| a | b |\n|---|---|\n| 1 | 2 | 3 |\n\n## t :: matrix\n\n| a |\n|---|\n| 1 |\n";
    let notes = hardcalc::parse::notes(src);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0].line, 5);
    assert!(notes[0].message.contains("3 cells"));
    assert!(notes[1].message.contains("used before"));
    assert!(hardcalc::parse::notes("| a | b |\n|---|---|\n| 1 | 2 |\n").is_empty());
}

#[test]
fn charts_name_columns_like_formulas() {
    let src = "## T :: matrix\n\n| k | v |\n|---|---|\n| a | 1 |\n| b | 2 |\n";
    let (mut wb, _) = hardcalc::Workbook::from_document(&parse(src).unwrap());
    for data in ["T!v", "T.v"] {
        let lines = vec!["type: bar".to_string(), format!("data: {data}"), "labels: T!k".to_string()];
        let spec = hardcalc::chart::ChartSpec::parse("C", &lines).unwrap();
        let chart = hardcalc::chart::resolve(&spec, &mut wb).unwrap();
        assert_eq!(chart.series.len(), 1, "{data}");
    }
}
