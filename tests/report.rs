use std::path::Path;

const BEAM: &str = include_str!("../examples/beam.hcmd");

#[test]
fn report_source_typesets_the_sheet() {
    let doc = hardcalc::parse(BEAM).unwrap();
    let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
    let title = hardcalc::report::title(&doc, Path::new("beam.hcmd"));
    assert_eq!(title, "Simply supported beam");
    let src = hardcalc::report::typst_source(&doc, &mut wb, &title);
    assert!(src.contains("$ f_\"yd\" = (f_\"yk\")/(gamma_\"M\") = (235)/(1.1) = 214 thin \"N/mm²\" $"), "{src}");
    assert!(src.contains("eta = 0.45 quad checkmark"), "{src}");
}

#[cfg(feature = "typst")]
#[test]
fn report_compiles_to_pdf() {
    let doc = hardcalc::parse(BEAM).unwrap();
    let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
    let src = hardcalc::report::typst_source(&doc, &mut wb, "Beam");
    let pdf = hardcalc::report::pdf(&src).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
}
