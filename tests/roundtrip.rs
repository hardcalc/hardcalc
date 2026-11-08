use hardcalc::model::*;
use hardcalc::{parse, write};

const BUDGET: &str = include_str!("../examples/budget.hcmd");

#[test]
fn canonical_form_is_idempotent() {
    let once = write(&parse(BUDGET).unwrap());
    let twice = write(&parse(&once).unwrap());
    assert_eq!(once, twice);
}

#[test]
fn roundtrip_preserves_model() {
    let doc = parse(BUDGET).unwrap();
    assert_eq!(parse(&write(&doc)).unwrap(), doc);
}

#[test]
fn recognizes_blocks() {
    let doc = parse(BUDGET).unwrap();
    let tables: Vec<&Table> = doc
        .blocks
        .iter()
        .filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None })
        .collect();
    assert_eq!(tables.len(), 4);

    let ausgaben = tables[0];
    assert_eq!(ausgaben.name.as_deref(), Some("Expenses"));
    assert_eq!(ausgaben.shape, Shape::Matrix(vec![Dim::Any, Dim::Fixed(5)]));
    assert_eq!(ausgaben.columns[1].name, "Amount");
    assert_eq!(ausgaben.columns[1].ty.as_deref(), Some("Euro"));
    assert_eq!(ausgaben.columns[1].align, Align::Right);
    assert_eq!(ausgaben.rows.len(), 3);
    assert_eq!(ausgaben.rows[0][3], "=B1/sum(B)");

    let umsatz = tables[1];
    assert_eq!(umsatz.shape.rank(), 3);

    assert_eq!(tables[2].shape, Shape::Vector);
    assert!(tables[3].name.is_none());

    let layers: Vec<&Layer> = doc
        .blocks
        .iter()
        .filter_map(|b| if let Block::Layer(l) = b { Some(l) } else { None })
        .collect();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].name, "format");
    assert_eq!(
        layers[0].rules[2],
        Rule::Rule { target: "row[Status=open]".into(), body: "fg yellow".into() }
    );
}

#[test]
fn escaped_pipes_survive() {
    let src = "| a | b |\n|---|---|\n| x \\| y | z |\n";
    let doc = parse(src).unwrap();
    let Block::Table(t) = &doc.blocks[0] else { panic!() };
    assert_eq!(t.rows[0][0], "x | y");
    assert!(write(&doc).contains("x \\| y"));
}

#[test]
fn layer_with_explicit_target() {
    let doc = parse("```layer links @Sales\nA1: https://example.com\n```\n").unwrap();
    let Block::Layer(l) = &doc.blocks[0] else { panic!() };
    assert_eq!(l.target.as_deref(), Some("Sales"));
}

#[test]
fn rejects_bad_shape() {
    assert!(parse("## X :: cube\n\n| a |\n|---|\n").is_err());
}

#[test]
fn compact_tables_are_written_without_padding() {
    let src = "## T :: matrix compact [Jan]\n\n| Item | Amount |\n|------|-------:|\n| Rent | 1200.00 |\n| A much longer item | 5 |\n";
    let doc = hardcalc::parse(src).unwrap();
    let out = hardcalc::write(&doc);
    assert_eq!(out, "## T :: matrix compact [Jan]\n\n| Item | Amount |\n|---|--:|\n| Rent | 1200.00 |\n| A much longer item | 5 |\n");
    // Reading it again changes nothing, and a longer value touches only its own line.
    assert_eq!(hardcalc::write(&hardcalc::parse(&out).unwrap()), out);
    let mut edited = hardcalc::parse(&out).unwrap();
    if let hardcalc::model::Block::Table(t) = &mut edited.blocks[0] {
        t.rows[0][0] = "A rent that got very long".into();
    }
    let changed = hardcalc::write(&edited);
    let diff = out.lines().zip(changed.lines()).filter(|(a, b)| a != b).count();
    assert_eq!(diff, 1, "{changed}");
}

#[test]
fn padded_tables_stay_padded() {
    let src = "## T :: matrix\n\n| Item | Amount |\n|---|---|\n| Rent | 1200.00 |\n";
    let out = hardcalc::write(&hardcalc::parse(src).unwrap());
    assert!(out.contains("| Item | Amount  |"), "{out}");
    assert!(!out.contains("compact"));
}
