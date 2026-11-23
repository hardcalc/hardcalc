//! Case distinctions in the calculation sheet: `χ =` with indented branches.

use hardcalc::sheet::{self, Out};

fn lines(src: &str) -> Vec<String> {
    src.lines().map(str::to_string).collect()
}

const BUCKLING: &str = "\
λ = 0.4
χ =
    λ ≤ 0.2: 1
    λ ≤ 1.0:
        Φ = 0.5·(1 + 0.34·(λ - 0.2) + λ^2)
        1/(Φ + sqrt(Φ^2 - λ^2))
    else: 0.5/λ^2
k = χ·2";

fn value(out: &Out) -> f64 {
    match out.inner() {
        Out::Assign { result: Ok(q), .. } | Out::Case { result: Ok(q), .. } => q.value,
        o => panic!("no value: {o:?}"),
    }
}

#[test]
fn the_first_branch_that_holds_is_computed() {
    let out = sheet::evaluate(&lines(BUCKLING), None);
    // One result per line.
    assert_eq!(out.len(), 8);
    assert!((value(&out[1]) - 0.926).abs() < 1e-3, "{:?}", out[1]);
    assert!((value(&out[7]) - 2.0 * value(&out[1])).abs() < 1e-9);
    assert!(sheet::problems(&out).is_empty(), "{:?}", sheet::problems(&out));
    // The branch after the one taken is not computed.
    assert!(matches!(out[6].inner(), Out::Skipped(_)));
    assert_eq!(out[1].taken().map(|b| b.is_else), Some(false));
}

#[test]
fn side_calculations_are_local() {
    let mut src = lines(BUCKLING);
    src.push("Φ".into());
    let out = sheet::evaluate(&src, None);
    assert!(sheet::problems(&out).iter().any(|p| p.contains("Φ")), "{:?}", sheet::problems(&out));
}

#[test]
fn else_and_first_branch() {
    for (l, want) in [(0.1, 1.0), (2.0, 0.125)] {
        let mut src = lines(BUCKLING);
        src[0] = format!("λ = {l}");
        let out = sheet::evaluate(&src, None);
        assert!((value(&out[1]) - want).abs() < 1e-3, "λ = {l}: {:?}", out[1]);
    }
}

#[test]
fn chained_conditions_and_units() {
    let src = lines("L = 3 m\ns =\n    0 m < L ≤ 2 m: 1 kN\n    2 m < L ≤ 4 m: 2 kN\n    else: 3 kN");
    let out = sheet::evaluate(&src, None);
    let Out::Case { result: Ok(q), .. } = &out[1] else { panic!("{:?}", out[1]) };
    assert_eq!(q.value, 2000.0);
}

#[test]
fn no_branch_holds_is_an_error() {
    let out = sheet::evaluate(&lines("a = 5\nb =\n    a < 1: 1\n    a < 2: 2"), None);
    assert!(sheet::problems(&out).iter().any(|p| p.contains("no condition holds")), "{:?}", sheet::problems(&out));
    // Mismatching units in a condition are reported.
    let out = sheet::evaluate(&lines("a = 5 m\nb =\n    a < 1 s: 1\n    else: 2"), None);
    assert!(!sheet::problems(&out).is_empty());
}

#[test]
fn the_report_draws_the_brace() {
    let out = sheet::evaluate(&lines(BUCKLING), None);
    let typ = sheet::notation::typst_report("", &out);
    assert!(typ.contains("cases("), "{typ}");
    assert!(typ.contains("\"if\"") || typ.contains("\"für\""), "{typ}");
    let lines = sheet::document_lines(&out);
    // The lines of the branches are not repeated; the side calculation follows the head.
    assert_eq!(lines.len(), 4);
    assert!(matches!(lines[2], Out::Assign { .. }));
}

#[test]
fn the_preview_draws_a_brace_with_term_maths() {
    let out = sheet::evaluate(&lines(BUCKLING), None);
    let latex = sheet::notation::line(sheet::notation::Lang::Latex, &out[1]).unwrap();
    let drawn = term_maths::render(&latex).to_string();
    eprintln!("{latex}\n{drawn}");
    assert!(drawn.lines().count() >= 3, "{drawn}");
}

#[test]
fn indent_survives_reading_and_writing() {
    let text = "<!-- hardcalc 1 -->\n\n```hardcalc\nχ =\n    λ ≤ 1,0: 2\n    else: 3\n```\n";
    let doc = hardcalc::parse(text).unwrap();
    let out = hardcalc::write(&doc);
    assert!(out.contains("\n    λ ≤ 1.0: 2\n    else: 3\n"), "{out}");
}
