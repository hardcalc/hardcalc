use hardcalc::model::{Block, Rule};
use std::process::ExitCode;

const USAGE: &str = "\
usage: hardcalc                   new document (a playground) in the terminal UI
       hardcalc <file>            open the file in the terminal UI
       hardcalc <command> <file>
       hardcalc desktop install    start menu entry and .hcmd file association
       hardcalc desktop uninstall  remove them again
       hardcalc --version
       hardcalc --keymap-markdown   the key / mouse matrix (docs/keys.md)

commands:
  fmt    print the file in canonical form
  check  print a summary and all type/formula errors with their line (file:line:);
         hardcalc check <file> --json prints them as JSON
  eval   print the file with formulas replaced by their values;
         hardcalc eval <file> -e <expr> [-e <expr>] [--raw] [--json] prints single results
  calc   print the calculation sheet (```hardcalc blocks) with the way to each result;
         hardcalc calc <file> --ascii writes plain ASCII with a decimal point
  report write the calculation sheet as PDF report with typeset formulas (<name>.report.pdf);
         hardcalc report <file> <out.pdf|out.typ> chooses the name, .typ writes the Typst source
  import read a spreadsheet (xlsx, xlsm, xlsb, xls, ods, csv, tsv) into <name>.hcmd;
         hardcalc import <file> <out.hcmd> chooses the name
  export write the document as spreadsheet: hardcalc export <file> <out.xlsx|.ods|.csv|.tsv>
         (CSV: one file per table, values only)
  pdf    write the document as PDF next to the file (<name>.pdf);
         hardcalc pdf <file> <Layout> prints a raster layout instead";

fn main() -> ExitCode {
    hardcalc::locale::init_from_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, path) = match args.as_slice() {
        [flag] if flag == "--keymap-markdown" => {
            print!("{}", hardcalc::tui::keymap::markdown());
            return ExitCode::SUCCESS;
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("hardcalc {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [] => {
            return match hardcalc::tui::run(None) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
        [path] if !path.starts_with('-') => {
            return match hardcalc::tui::run(Some(std::path::Path::new(path))) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
        [cmd, action] if cmd == "desktop" && (action == "install" || action == "uninstall") => {
            let result = if action == "install" { hardcalc::desktop::install() } else { hardcalc::desktop::uninstall() };
            return match result {
                Ok(lines) => {
                    for line in lines {
                        println!("{line}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
        [cmd, path, ..] if cmd == "check" || cmd == "eval" => (cmd.as_str(), path),
        [cmd, path] => (cmd.as_str(), path),
        [cmd, path, _] if cmd == "pdf" || cmd == "import" || cmd == "export" || cmd == "calc" || cmd == "report" => (cmd.as_str(), path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    if cmd == "import" {
        let input = std::path::Path::new(path);
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| hardcalc::import::target_path(input));
        if out.exists() {
            eprintln!("{} exists – choose another name", out.display());
            return ExitCode::FAILURE;
        }
        return match hardcalc::import::import_file(input) {
            Ok((doc, report)) => match std::fs::write(&out, hardcalc::write(&doc)) {
                Ok(()) => {
                    println!("{} – {report}", out.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}: {e}", out.display());
                    ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if cmd == "export" {
        let Some(out) = args.get(2) else {
            eprintln!("usage: hardcalc export <file.hcmd> <out.xlsx>");
            return ExitCode::from(2);
        };
        let result = hardcalc::storage::read_text(std::path::Path::new(path))
            .map(|(s, _)| s)
            .map_err(|e| format!("{path}: {e}"))
            .and_then(|src| hardcalc::parse(&src).map_err(|e| format!("{path}: {e}")))
            .and_then(|doc| hardcalc::export::export_file(&doc, std::path::Path::new(out)));
        return match result {
            Ok(report) => {
                println!("{out} – {report}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if cmd == "pdf" {
        let layout = args.get(2);
        let result = hardcalc::tui::app::App::open(std::path::Path::new(path))
            .map_err(|e| e.to_string())
            .and_then(|mut app| {
                let scope = match layout {
                    None => hardcalc::print::Scope::Document,
                    Some(name) => match app.layout_blocks().iter().position(|(n, _)| n == name) {
                        Some(i) => hardcalc::print::Scope::Layout(i),
                        None => return Err(format!("no layout named '{name}'")),
                    },
                };
                let setup = hardcalc::print::PageSetup { scope, ..Default::default() };
                app.write_pdf(&setup, None)
            });
        return match result {
            Ok((out, pages)) => {
                println!("{} ({pages} page{})", out.display(), if pages == 1 { "" } else { "s" });
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    let src = match hardcalc::storage::read_text(std::path::Path::new(path)) {
        Ok((s, _)) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let doc = match hardcalc::parse(&src) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cmd {
        "fmt" => print!("{}", hardcalc::write(&doc)),
        "check" => {
            let json = args[2..].iter().any(|a| a == "--json");
            if let Some(bad) = args[2..].iter().find(|a| *a != "--json") {
                eprintln!("check: unknown option '{bad}'\n{USAGE}");
                return ExitCode::from(2);
            }
            let mut findings: Vec<Finding> = Vec::new();
            let (mut wb, diags) = hardcalc::Workbook::from_document(&doc);
            for d in &diags {
                let message = match &d.cell {
                    Some(c) => d.message.strip_prefix(&format!("{c}: ")).unwrap_or(&d.message).to_string(),
                    None => d.message.clone(),
                };
                let (block, table) = if d.cell.is_some() { ("table".to_string(), Some(d.location.clone())) } else { (d.location.clone(), None) };
                findings.push(Finding { severity: "error", line: d.line, block, table, cell: d.cell.clone(), message });
            }
            let attached = hardcalc::layers::attach(&doc, &wb);
            let formats = hardcalc::layers::Formats::new(&attached, "format");
            let layer_finding = |e: &hardcalc::layers::LayerError, wb: &hardcalc::Workbook| Finding {
                severity: "error",
                line: e.line,
                block: format!("layer {}", e.layer),
                table: wb.sheets[e.sheet].name.clone(),
                cell: None,
                message: e.message.clone(),
            };
            for e in &formats.errors {
                findings.push(layer_finding(e, &wb));
            }
            let (violations, rule_errors) = hardcalc::layers::validate(&attached, &mut wb, "validate");
            for e in &rule_errors {
                findings.push(layer_finding(e, &wb));
            }
            for v in &violations {
                let (s, r, c) = v.cell;
                findings.push(Finding {
                    severity: "invalid",
                    line: 0,
                    block: "layer validate".into(),
                    table: Some(wb.sheets[s].name.clone().unwrap_or_else(|| format!("table #{}", s + 1))),
                    cell: Some(format!("{}{}", hardcalc::engine::col_letters(c), r + 1)),
                    message: v.message.clone(),
                });
            }
            for block in &doc.blocks {
                if let Block::Chart { name, lines } = block {
                    let result = hardcalc::chart::ChartSpec::parse(name, lines)
                        .and_then(|spec| hardcalc::chart::resolve(&spec, &mut wb));
                    if let Err(e) = result {
                        findings.push(Finding { severity: "error", line: 0, block: format!("chart {name}"), table: None, cell: None, message: e.to_string() });
                    }
                }
            }
            let calc_problems = hardcalc::sheet::problems(&hardcalc::sheet::evaluate_document(&doc, &mut wb));
            for p in &calc_problems {
                findings.push(Finding { severity: "error", line: 0, block: "calc".into(), table: None, cell: None, message: p.to_string() });
            }
            if json {
                println!("{}", check_json(&doc, &findings));
            } else {
                summarize(&doc);
                for f in &findings {
                    let place = if f.line > 0 { format!("{path}:{}: ", f.line) } else { String::new() };
                    let table = f.table.as_deref().map(|t| format!(" @{t}")).unwrap_or_default();
                    let text = match (&f.cell, f.block.as_str()) {
                        (Some(c), "table") => format!("{}: {c}: {}", f.table.as_deref().unwrap_or(""), f.message),
                        (Some(c), _) => format!("{}{table}: {c}: {}", f.block, f.message),
                        (None, _) => format!("{}{table}: {}", f.block, f.message),
                    };
                    println!("{place}{}  {text}", f.severity);
                }
            }
            if !findings.is_empty() {
                return ExitCode::FAILURE;
            }
        }
        "calc" => {
            let ascii = args.get(2).is_some_and(|a| a == "--ascii");
            let render = hardcalc::sheet::Render { style: hardcalc::sheet::expr::TextStyle { ascii, point: ascii || !hardcalc::locale::decimal_comma() } };
            let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
            let out = hardcalc::sheet::evaluate_document(&doc, &mut wb);
            if out.is_empty() {
                eprintln!("{path}: no ```hardcalc block");
                return ExitCode::FAILURE;
            }
            for line in render.lines(&out) {
                println!("{line}");
            }
            if !hardcalc::sheet::problems(&out).is_empty() {
                return ExitCode::FAILURE;
            }
        }
        "report" => {
            let p = std::path::Path::new(path);
            let out = match args.get(2) {
                Some(o) => std::path::PathBuf::from(o),
                None => {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let stem = hardcalc::storage::document_stem(&name);
                    p.with_file_name(format!("{stem}.report.pdf"))
                }
            };
            let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
            if hardcalc::sheet::source_lines(&doc).is_empty() {
                eprintln!("{path}: no ```hardcalc block");
                return ExitCode::FAILURE;
            }
            let source = hardcalc::report::typst_source(&doc, &mut wb, &hardcalc::report::title(&doc, p));
            let result = if out.extension().is_some_and(|e| e == "typ") {
                std::fs::write(&out, source).map_err(|e| format!("{}: {e}", out.display()))
            } else {
                hardcalc::report::compile(&source, &out)
            };
            match result {
                Ok(()) => println!("{}", out.display()),
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        "eval" => {
            let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
            let mut exprs = Vec::new();
            let mut raw = false;
            let mut json = false;
            let mut rest = args[2..].iter();
            while let Some(a) = rest.next() {
                match a.as_str() {
                    "-e" => match rest.next() {
                        Some(e) => exprs.push(e.clone()),
                        None => {
                            eprintln!("eval: -e needs an expression");
                            return ExitCode::from(2);
                        }
                    },
                    "--raw" => raw = true,
                    "--json" => json = true,
                    other => {
                        eprintln!("eval: unknown option '{other}'\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            if exprs.is_empty() {
                if raw || json {
                    eprintln!("eval: --raw and --json need -e <expression>");
                    return ExitCode::from(2);
                }
                print!("{}", hardcalc::write(&wb.render(&doc)));
                return ExitCode::SUCCESS;
            }
            let mut failed = false;
            let mut results = Vec::new();
            for e in &exprs {
                let v = wb.eval_formula(e, 0);
                failed |= v.is_error();
                if json {
                    let (value, unit) = match &v {
                        hardcalc::value::Value::Num(n, u) => (serde_json::json!(n), (!u.is_none()).then(|| u.to_string())),
                        hardcalc::value::Value::Bool(b) => (serde_json::json!(b), None),
                        hardcalc::value::Value::Error(_) | hardcalc::value::Value::Null => (serde_json::Value::Null, None),
                        other => (serde_json::json!(other.display(None, false)), None),
                    };
                    let error = if let hardcalc::value::Value::Error(m) = &v { Some(m.clone()) } else { None };
                    results.push(serde_json::json!({"expr": e, "value": value, "unit": unit, "error": error}));
                } else if raw {
                    match &v {
                        hardcalc::value::Value::Num(n, _) => println!("{n}"),
                        other => println!("{}", other.display(None, false)),
                    }
                } else {
                    println!("{}", v.display(None, wb.decimal_comma));
                }
            }
            if json {
                let out = if results.len() == 1 { results.remove(0) } else { serde_json::Value::Array(results) };
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            }
            if failed {
                return ExitCode::FAILURE;
            }
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn summarize(doc: &hardcalc::Document) {
    for block in &doc.blocks {
        match block {
            Block::Table(t) => {
                let name = t.name.as_deref().unwrap_or("(anonymous)");
                let cols: Vec<String> = t
                    .columns
                    .iter()
                    .map(|c| match &c.ty {
                        Some(ty) => format!("{}:{}", c.name, ty),
                        None => c.name.clone(),
                    })
                    .collect();
                println!("table  {name} :: {} — {} rows × {} cols [{}]", t.shape, t.rows.len(), t.columns.len(), cols.join(", "));
            }
            Block::Types(lines) => {
                let n = lines.iter().filter(|l| l.contains('=')).count();
                println!("types  {n} definitions");
            }
            Block::Layer(l) => {
                let n = l.rules.iter().filter(|r| matches!(r, Rule::Rule { .. })).count();
                let target = l.target.as_deref().map(|t| format!(" @{t}")).unwrap_or_default();
                println!("layer  {}{target} — {n} rules", l.name);
            }
            Block::Chart { name, .. } => println!("chart  {name}"),
            Block::Functions(lines) => {
                let (defs, _) = hardcalc::functions::parse_block(lines);
                println!("functions {}", defs.iter().map(|(_, d)| d.name.as_str()).collect::<Vec<_>>().join(", "));
            }
            Block::Layout { name, lines } => println!("layout {name} — {} lines", lines.len()),
            Block::Text(_) | Block::Code { .. } => {}
        }
    }
}

/// One problem `check` found.
struct Finding {
    severity: &'static str,
    line: usize,
    block: String,
    table: Option<String>,
    cell: Option<String>,
    message: String,
}

fn check_json(doc: &hardcalc::Document, findings: &[Finding]) -> String {
    let tables: Vec<serde_json::Value> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(serde_json::json!({
                "name": t.name,
                "shape": t.shape.to_string(),
                "rows": t.rows.len(),
                "columns": t.columns.iter().map(|c| match &c.ty {
                    Some(ty) => format!("{}:{}", c.name, ty),
                    None => c.name.clone(),
                }).collect::<Vec<_>>(),
            })),
            _ => None,
        })
        .collect();
    let diagnostics: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "severity": f.severity,
                "line": (f.line > 0).then_some(f.line),
                "block": f.block,
                "table": f.table,
                "cell": f.cell,
                "message": f.message,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({"tables": tables, "diagnostics": diagnostics})).unwrap_or_default()
}
