use hardcalc::tui::app::{App, Header, Hit, Mode};
use hardcalc::tui::ui;
use hardcalc::units::Unit;
use hardcalc::value::Value;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use std::path::PathBuf;

const BUDGET: &str = include_str!("../examples/budget.hcmd");

fn app() -> App {
    App::new(PathBuf::from("budget.hcmd"), hardcalc::parse(BUDGET).unwrap())
}

fn render(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, ch: char) {
    app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn typ(app: &mut App, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}

fn click(app: &mut App, target: Hit) {
    let (rect, _) = *app.hits.iter().find(|(_, h)| *h == target).unwrap_or_else(|| panic!("no hit {target:?}"));
    for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
        app.on_mouse(MouseEvent { kind, column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
    }
}

#[test]
fn renders_values_formats_and_layers() {
    let mut a = app();
    let screen = render(&mut a);
    assert!(screen.contains("Expenses"), "{screen}");
    assert!(screen.contains("1200.00"));
    // format layer: `D: percent(1)`
    assert!(screen.contains("90.5 %"), "{screen}");
    assert!(screen.contains("■ format"));
    assert!(screen.contains("■ validate"));
}

#[test]
fn mouse_selects_and_toggles_layers() {
    let mut a = app();
    render(&mut a);
    click(&mut a, Hit::Cell(1, 1));
    assert_eq!(a.view.cursor, (1, 1));
    assert_eq!(a.cursor_raw(), "85.50");

    click(&mut a, Hit::Layer(1)); // 0 is the built-in formulas layer
    assert!(!a.layer_visible("format"));
    let screen = render(&mut a);
    assert!(!screen.contains("90.5 %"));
    assert!(screen.contains("□ format"));

    click(&mut a, Hit::Tab(2));
    assert_eq!(a.view.sheet, 2);
}

#[test]
fn edit_recalculates_and_undo_restores() {
    let mut a = app();
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    typ(&mut a, "100");
    assert!(matches!(a.mode, Mode::Edit { .. }));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(a.view.cursor, (2, 1));
    assert!(a.dirty);
    // Gross = Amount * 1.19
    assert_eq!(a.wb.value((0, 1, 4)), Value::Num(119.0, Unit::symbol("€")));

    ctrl(&mut a, 'z');
    assert_eq!(a.wb.sheets[0].raw[1][1], "85.50");
    ctrl(&mut a, 'y');
    assert_eq!(a.wb.sheets[0].raw[1][1], "100");
}

#[test]
fn validation_flags_cells() {
    let mut a = app();
    typ(&mut a, ":goto B1");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "-5");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.violations.get(&(0, 0, 1)).map(String::as_str), Some("must be >= 0"));
    let screen = render(&mut a);
    assert!(screen.contains("1 violation"), "{screen}");
}

#[test]
fn copy_paste_and_commands() {
    let mut a = app();
    // Select A1:A2 with Shift+Down and copy into column A rows 3–4 after adding a row.
    a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    ctrl(&mut a, 'c');
    typ(&mut a, ":addrow");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.dims().0, 4);
    typ(&mut a, ":A3");
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'v');
    assert_eq!(a.wb.sheets[0].raw[2][0], "Rent");
    assert_eq!(a.wb.sheets[0].raw[3][0], "Power");

    typ(&mut a, ":sort Amount desc");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[0].raw[0][0], "Rent");

    typ(&mut a, ":addcol Note:text");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.table().columns[1].name, "Note");
}

#[test]
fn pivot_view_of_tensor() {
    let mut a = app();
    a.switch_sheet(1);
    ctrl(&mut a, 'p');
    assert!(a.view.pivot.is_some());
    // rows: Month (Jan, Feb), columns: Region (North, South), Product fixed to A
    assert_eq!(a.dims(), (2, 2));
    let screen = render(&mut a);
    assert!(screen.contains("Product = A"), "{screen}");
    assert_eq!(a.cell_view(0, 0).text, "120.00");
    // Feb / South / A does not exist yet: typing creates a new tensor row.
    assert_eq!(a.cell_view(1, 1).text, "");
    a.view.cursor = (1, 1);
    typ(&mut a, "99");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[1].raw.last().unwrap(), &vec!["Feb", "South", "A", "99"]);
    assert_eq!(a.cell_view(1, 1).text, "99.00");

    click(&mut a, Hit::Filter(2));
    assert_eq!(a.view.pivot.as_ref().unwrap().fixed[2], 1);
    assert_eq!(a.cell_view(1, 1).text, "42.10");
}

#[test]
fn save_round_trips_and_quit_guard() {
    let dir = std::env::temp_dir().join(format!("hardcalc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.hcmd");
    let mut a = App::open(&path).unwrap(); // does not exist yet → new document
    typ(&mut a, "=2*21");
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'q');
    assert!(!a.quit, "dirty document must not quit");
    ctrl(&mut a, 's');
    assert!(!a.dirty);
    ctrl(&mut a, 'q');
    assert!(a.quit);

    let mut b = App::open(&path).unwrap();
    assert_eq!(b.wb.value((0, 0, 0)), Value::num(42.0));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn placeholders_grow_the_table() {
    let mut a = app();
    let screen = render(&mut a);
    assert!(screen.contains(" + "), "{screen}");
    assert_eq!(a.dims(), (3, 5));
    assert_eq!(a.view_dims(), (4, 6));

    // Type into the placeholder row …
    click(&mut a, Hit::Cell(3, 0));
    typ(&mut a, "Wasser");
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.dims(), (4, 5));
    assert_eq!(a.wb.sheets[0].raw[3][0], "Wasser");
    // … and into the placeholder column (named after its letter).
    a.view.cursor = (0, 5);
    typ(&mut a, "x");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.dims(), (4, 6));
    assert_eq!(a.table().columns[5].name, "F");
    assert_eq!(a.table().shape.to_string(), "matrix[*, 6]");

    // Growing and writing is a single undo step.
    ctrl(&mut a, 'z');
    assert_eq!(a.dims(), (4, 5));
    ctrl(&mut a, 'z');
    assert_eq!(a.dims(), (3, 5));

    // Clearing or empty input in a placeholder does not grow anything.
    a.view.cursor = (3, 2);
    key(&mut a, KeyCode::Delete);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.dims(), (3, 5));
    typ(&mut a, ":delrow");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.dims(), (3, 5));
}

#[test]
fn paste_beyond_the_edge_grows() {
    let mut a = app();
    a.view.cursor = (2, 4);
    a.on_paste("1\t2\n3\t4\n");
    assert_eq!(a.dims(), (4, 6));
    assert_eq!(a.wb.sheets[0].raw[3][5], "4");
}

#[test]
fn sort_toggles_on_repeat() {
    let mut a = app();
    let col0 = |a: &App| a.wb.sheets[0].raw.iter().map(|r| r[0].clone()).collect::<Vec<_>>();
    a.view.cursor = (0, 1); // Amount
    let sort = |a: &mut App| {
        typ(a, ":sort");
        key(a, KeyCode::Enter);
    };
    sort(&mut a);
    assert_eq!(col0(&a), ["Internet", "Power", "Rent"]);
    sort(&mut a);
    assert_eq!(col0(&a), ["Rent", "Power", "Internet"]);
    sort(&mut a);
    assert_eq!(col0(&a), ["Internet", "Power", "Rent"]);

    // Any other action in between restarts with ascending.
    sort(&mut a);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Up);
    sort(&mut a);
    assert_eq!(col0(&a), ["Internet", "Power", "Rent"]);

    // Text columns sort alphabetically.
    a.view.cursor = (0, 0);
    sort(&mut a);
    assert_eq!(col0(&a), ["Internet", "Power", "Rent"]);
}

fn fkey(app: &mut App, n: u8) {
    app.on_key(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE));
}

#[test]
fn types_overview() {
    let mut a = app();
    fkey(&mut a, 3);
    assert!(a.types_view());
    let screen = render(&mut a);
    for s in ["⚙ types", "Definition", "Euro", "decimal(2)", "€", "Expenses.Amount", "enum (3 values)"] {
        assert!(screen.contains(s), "missing {s:?}\n{screen}");
    }

    // Edit the definition of Euro: values are re-rendered with one decimal.
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Backspace); // `decimal(2) unit "€"` → edit in place
    for _ in 0..9 {
        key(&mut a, KeyCode::Left);
    }
    key(&mut a, KeyCode::Backspace);
    typ(&mut a, "1");
    key(&mut a, KeyCode::End);
    typ(&mut a, "\"");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.cursor_problem(), None);
    assert!(hardcalc::write(&a.doc).contains("Euro   = decimal(1) unit \"€\"") || hardcalc::write(&a.doc).contains("Euro = decimal(1) unit \"€\""));
    fkey(&mut a, 3);
    assert!(!a.types_view());
    assert!(render(&mut a).contains("1200.0 "));

    // Add a type through the + row, then its definition.
    fkey(&mut a, 3);
    a.view.cursor = (2, 0);
    typ(&mut a, "Prozent");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view.cursor, (2, 1), "continues with the definition");
    key(&mut a, KeyCode::Enter);
    for _ in 0..4 {
        key(&mut a, KeyCode::Backspace);
    }
    typ(&mut a, "decimal(1) unit \"%\"");
    key(&mut a, KeyCode::Enter);
    let screen = render(&mut a);
    assert!(screen.contains("Prozent") && screen.contains("decimal(1)"), "{screen}");

    // Invalid names and built-ins are rejected.
    a.view.cursor = (3, 0);
    typ(&mut a, "int");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().is_some_and(|m| m.error));
    a.view.cursor = (5, 1);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.mode, Mode::Normal, "built-ins are read-only");

    // F8 deletes, undo restores.
    a.view.cursor = (2, 0);
    fkey(&mut a, 8);
    assert!(!hardcalc::write(&a.doc).contains("Prozent"));
    ctrl(&mut a, 'z');
    assert!(hardcalc::write(&a.doc).contains("Prozent = decimal(1) unit \"%\""));
}

#[test]
fn type_help_while_editing() {
    let mut a = app();
    fkey(&mut a, 3);
    a.view.cursor = (0, 1);
    let draw = |a: &mut App| {
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| ui::draw(f, a)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!draw(&mut a).contains("Modifiers"));
    key(&mut a, KeyCode::Enter);
    let screen = draw(&mut a);
    for s in ["type definition", "Modifiers", "check iban", "unit \"€\"", "Euro"] {
        assert!(screen.contains(s), "missing {s:?}\n{screen}");
    }
    key(&mut a, KeyCode::Esc);
    assert!(!draw(&mut a).contains("Modifiers"));
}

#[test]
fn broken_type_definition_is_reported() {
    let mut a = app();
    fkey(&mut a, 3);
    a.view.cursor = (1, 1); // Status
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "x");
    key(&mut a, KeyCode::Home);
    typ(&mut a, "bogus ");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (1, 2);
    assert!(a.cursor_problem().is_some_and(|p| p.starts_with("Status: ")));
    assert!(render(&mut a).contains("#ERR"));
}

#[test]
fn input_is_visible_in_the_cell() {
    let mut a = app();
    a.view.cursor = (3, 0); // placeholder row
    typ(&mut a, "hello");
    let screen = render(&mut a);
    let row = screen.lines().find(|l| l.trim_start().starts_with('+')).unwrap();
    assert!(row.contains("hello"), "{screen}");

    // Too long for the cell: the most recently typed part is shown.
    typ(&mut a, "_world_this_is_long_END");
    let screen = render(&mut a);
    let row = screen.lines().find(|l| l.trim_start().starts_with('+')).unwrap();
    assert!(row.contains("ong_END"), "{row}");
    assert!(!row.contains("hello"), "{row}");
}

#[test]
fn function_key_bar() {
    let mut a = app();
    let screen = render(&mut a);
    let last = screen.lines().last().unwrap();
    for s in ["1Help", "2Save", "3Types", "7Sort", "10Quit"] {
        assert!(last.contains(s), "{last}");
    }
    // Clicking F7 twice sorts ascending, then descending.
    a.view.cursor = (0, 1);
    click(&mut a, Hit::FKey(7));
    assert_eq!(a.wb.sheets[0].raw[0][0], "Internet");
    render(&mut a);
    click(&mut a, Hit::FKey(7));
    assert_eq!(a.wb.sheets[0].raw[0][0], "Rent");

    // Quitting with unsaved changes needs a second F10.
    render(&mut a);
    click(&mut a, Hit::FKey(10));
    assert!(!a.quit);
    fkey(&mut a, 10);
    assert!(a.quit);
}

#[test]
fn pivot_button() {
    let mut a = app();
    assert!(!render(&mut a).contains("⊞ pivot"));
    click(&mut a, Hit::Tab(1));
    assert!(render(&mut a).contains("⊞ pivot"));
    click(&mut a, Hit::PivotButton);
    assert!(a.view.pivot.is_some());
}

#[test]
fn formulas_layer_marks_formula_cells() {
    use hardcalc::layers::Color;
    let mut a = app();
    assert_eq!(a.layer_names[0], "formulas");
    let formula = a.cell_view(0, 3); // =B1/sum(B)
    let data = a.cell_view(0, 1);
    assert!(formula.style.italic && formula.style.fg == Some(Color::Named("cyan".into())));
    assert!(!data.style.italic && data.style.fg.is_none());
    assert!(render(&mut a).contains("■ formulas"));

    click(&mut a, Hit::Layer(0));
    assert!(!a.cell_view(0, 3).style.italic);

    // A `layer formulas` block replaces the default marker.
    let src = "| a | b |\n|---|---|\n| 1 | =A1*2 |\n\n```layer formulas\n*: fg magenta\n```\n";
    let mut b = App::new(PathBuf::from("x.hcmd"), hardcalc::parse(src).unwrap());
    assert_eq!(b.cell_view(0, 1).style.fg, Some(Color::Named("magenta".into())));
    assert_eq!(b.cell_view(0, 0).style.fg, None);
}

#[test]
fn print_preview() {
    let dir = std::env::temp_dir().join(format!("hardcalc-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = App::new(dir.join("budget.hcmd"), hardcalc::parse(BUDGET).unwrap());
    assert_eq!(a.pdf_path(), dir.join("budget.pdf"));

    command(&mut a, "print");
    assert!(a.preview.is_some());
    let screen = render(&mut a);
    assert!(screen.contains("2PDF") && screen.contains("10Close"), "{screen}");
    assert!(screen.contains("page 1/1"), "{screen}");

    let size = |a: &App| a.preview.as_ref().unwrap().layout.font_size;
    let auto = size(&a);
    fkey(&mut a, 7);
    assert_eq!(size(&a), auto.round() - 1.0);
    fkey(&mut a, 5);
    assert!(a.print_setup.landscape);
    fkey(&mut a, 9);
    assert_eq!(a.print_setup.scope, hardcalc::print::Scope::Document);
    let tables = a.preview.as_ref().unwrap().layout.pages.iter().flatten().filter(|l| l.kind == hardcalc::print::LineKind::Title).count();
    assert!(tables >= 5, "title + 4 tables");

    // F2 writes the PDF next to the document.
    fkey(&mut a, 2);
    let pdf = std::fs::read(dir.join("budget.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF"));

    key(&mut a, KeyCode::Esc);
    assert!(a.preview.is_none());
    // Setup is remembered for the next preview.
    command(&mut a, "print");
    assert!(a.print_setup.landscape);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn inspector_text(a: &mut App) -> String {
    a.inspector().iter().map(|l| format!("{l:?}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn split_with_inspector() {
    use hardcalc::tui::app::{InspectorLine, ViewKind};
    let mut a = app();
    ctrl(&mut a, 'o');
    assert!(a.is_split());
    assert_eq!(a.other.as_ref().unwrap().kind, ViewKind::Inspector);
    assert!(a.left_focused);

    // Enum cell: the inspector offers its values.
    a.view.cursor = (1, 2); // Status of Power = open
    let lines = a.inspector();
    assert!(lines.contains(&InspectorLine::Choice { value: "open".into(), current: true }));
    assert!(lines.contains(&InspectorLine::Choice { value: "paid".into(), current: false }));

    // Tab focuses the inspector; Down + Enter picks "paid" for the main window's cell.
    key(&mut a, KeyCode::Tab);
    assert!(!a.left_focused);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.with_window(true, |a| a.cursor_raw()), "paid");
    // Typing in a helper window does not edit anything.
    typ(&mut a, "x");
    assert_eq!(a.mode, Mode::Normal);

    // Clicking a choice while the table is focused applies it as well.
    key(&mut a, KeyCode::Tab);
    assert!(a.left_focused);
    render(&mut a);
    let (rect, _) = *a.other_hits.iter().find(|(_, h)| *h == Hit::Choice(2)).unwrap();
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
    assert_eq!(a.with_window(true, |a| a.cursor_raw()), "cancelled");

    // Ctrl+U only swaps data windows; Ctrl+O closes and focuses the main window.
    ctrl(&mut a, 'u');
    assert!(a.message.as_ref().is_some_and(|m| m.error));
    ctrl(&mut a, 'o');
    assert!(!a.is_split() && a.left_focused && a.view.kind == ViewKind::Table);
}

#[test]
fn inspector_shows_formula_links() {
    let mut a = app();
    ctrl(&mut a, 'o');
    a.view.cursor = (0, 3); // Share = B1/sum(B)
    let text = inspector_text(&mut a);
    assert!(text.contains(r#"Field("Uses", "B1, B2, B3")"#), "{text}");
    a.view.cursor = (0, 1); // Amount of Rent
    let text = inspector_text(&mut a);
    assert!(text.contains(r#"Field("Used by", "D1, E1, D2, E2, D3, E3, Metrics!B2")"#), "{text}");
    assert!(text.contains(r#"Field("Format", "align right")"#), "{text}");
}

#[test]
fn source_view_follows_main_window() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    ctrl(&mut a, 'o');
    render(&mut a);
    // Helpers can only be chosen on the right: clicking the right window's tab focuses it.
    let (rect, _) = *a.other_hits.iter().find(|(_, h)| *h == Hit::ViewTab(ViewKind::Source)).unwrap();
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
    assert_eq!(a.view.kind, ViewKind::Source);
    let (lines, range) = a.source_lines();
    assert!(lines[range.start].starts_with("## Expenses"));
    assert!(lines[range.clone()].iter().any(|l| l == "```layer validate"), "attached layers are included");

    a.with_window(true, |a| a.switch_sheet(1));
    let (lines, range) = a.source_lines();
    assert!(lines[range.start].starts_with("## Sales"));
    let screen = render(&mut a);
    assert!(screen.contains("## Sales :: tensor"), "{screen}");
}

#[test]
fn two_tables_side_by_side_and_swap() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    ctrl(&mut a, 'o');
    key(&mut a, KeyCode::Tab);
    render(&mut a);
    click(&mut a, Hit::Tab(2)); // right window: Metrics
    assert_eq!((a.view.kind, a.view.sheet), (ViewKind::Table, 2));
    let screen = render(&mut a);
    assert!(screen.contains("Total expenses") && screen.contains("Internet"), "{screen}");

    // Ctrl+U swaps the windows; the focus stays with Metrics, now on the left.
    ctrl(&mut a, 'u');
    assert!(a.left_focused);
    assert_eq!(a.view.sheet, 2);
    assert_eq!(a.other.as_ref().unwrap().sheet, 0);

    // The left window never shows helpers.
    a.set_view(ViewKind::Table, 2);
    render(&mut a);
    // The left window never offers helpers (layouts are main views and may appear).
    assert!(!a.hits.iter().any(|(_, h)| matches!(h, Hit::ViewTab(ViewKind::Inspector | ViewKind::Source | ViewKind::Chart(_)))));
}

#[test]
fn charts_in_the_right_window() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    // The chart tab in the top bar opens the split with the chart on the right.
    render(&mut a);
    click(&mut a, Hit::ViewTab(ViewKind::Chart(0)));
    assert!(a.is_split() && a.left_focused);
    assert_eq!(a.other.as_ref().unwrap().kind, ViewKind::Chart(0));
    a.view.cursor = (1, 1);
    let screen = render(&mut a);
    assert!(screen.contains("Spending (bar)") && screen.contains("■ Gross"), "{screen}");
    assert!(screen.contains("101.75"), "{screen}");

    // The chart follows the table cursor; clicking a bar moves it.
    let data = a.chart_data(0).unwrap();
    assert_eq!(a.chart_highlight(&data), Some(1));
    let (rect, _) = *a.other_hits.iter().find(|(_, h)| *h == Hit::ChartItem(2)).unwrap();
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
    assert_eq!(a.with_window(true, |a| a.view.cursor.0), 2);

    // t switches the chart type and is written to the document; undo restores.
    assert!(!a.left_focused);
    key(&mut a, KeyCode::Char('t'));
    assert!(hardcalc::write(&a.doc).contains("type: line"));
    assert!(render(&mut a).contains("(line)"));
    ctrl(&mut a, 'z');
    assert!(hardcalc::write(&a.doc).contains("type:   bar"));
}

#[test]
fn create_chart_from_selection() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.view.cursor = (0, 1);
    a.view.anchor = Some((2, 4)); // Amount … Gross
    typ(&mut a, ":chart");
    key(&mut a, KeyCode::Enter);
    let src = hardcalc::write(&a.doc);
    assert!(src.contains("```chart Expenses chart\ntype: bar\ndata: Expenses.Amount, Expenses.Share, Expenses.Gross\nlabels: Expenses.Item\n```"), "{src}");
    // Mixed units (€ and none) are reported in the chart window.
    let i = a.chart_blocks().iter().position(|(n, _)| n == "Expenses chart").unwrap();
    assert_eq!(a.other.as_ref().unwrap().kind, ViewKind::Chart(i));
    assert!(render(&mut a).contains("unit mismatch"));

    // Tensor: a line chart over the first axis, series = second axis, filter for the rest.
    a.switch_sheet(1);
    typ(&mut a, ":chart");
    key(&mut a, KeyCode::Enter);
    let src = hardcalc::write(&a.doc);
    assert!(src.contains("```chart Sales chart\ntype: line\ndata: Sales\nx: Month\nseries: Region\nfilter: Product=A\n```"), "{src}");
}

#[test]
fn charts_are_printed() {
    let mut a = app();
    let items = a.print_items(false);
    assert_eq!(items.len(), 2, "current table and its chart");
    let all = a.print_items(true);
    let tables = all.iter().filter(|i| matches!(i, hardcalc::print::PrintItem::Table(_))).count();
    let texts = all.iter().filter(|i| matches!(i, hardcalc::print::PrintItem::Text(_))).count();
    assert_eq!((tables, all.len() - tables - texts), (4, 2), "4 tables + 2 charts");
    assert_eq!(texts, 3);
    let layout = a.layout(&hardcalc::print::PageSetup { scope: hardcalc::print::Scope::Document, ..Default::default() });
    let charts: usize = layout.graphics.iter().map(Vec::len).sum();
    assert_eq!(charts, 2);
    let pdf = hardcalc::print::to_pdf(&layout, "t");
    assert!(pdf.windows(2).any(|w| w == b"re"), "bars are drawn as rectangles");

    command(&mut a, "print");
    let screen = render(&mut a);
    assert!(screen.contains("Spending (bar)"), "{screen}");
}

#[test]
fn sparkline_formula() {
    let mut a = app();
    let k = a.wb.sheet_index("Metrics").unwrap();
    assert_eq!(a.wb.value((k, 6, 1)), hardcalc::value::Value::Text("█▁▁".into()));
}

#[test]
fn comments_as_footnotes() {
    let mut a = app();
    a.view.cursor = (0, 1);
    assert!(a.cell_view(0, 1).style.underline, "commented cell is marked");
    assert_eq!(a.cursor_comment().as_deref(), Some("Rent increase announced for July"));
    assert!(render(&mut a).contains("✎ Rent increase"));
    ctrl(&mut a, 'o');
    assert!(inspector_text(&mut a).contains(r#"Field("Comment", "Rent increase announced for July")"#));

    let items = a.print_items(false);
    let hardcalc::print::PrintItem::Table(t) = &items[0] else { panic!() };
    assert_eq!(t.rows[0][1].text, "1200.00¹");
    assert_eq!(t.footnotes, ["¹ Rent increase announced for July"]);

    // Hidden layer: no marker, no footnote.
    a.toggle_layer("comments");
    let items = a.print_items(false);
    let hardcalc::print::PrintItem::Table(t) = &items[0] else { panic!() };
    assert!(t.footnotes.is_empty() && t.rows[0][1].text == "1200.00");
}

#[test]
fn flow_layout_prints_the_document() {
    let mut a = app();
    let layout = a.layout(&hardcalc::print::PageSetup { scope: hardcalc::print::Scope::Document, ..Default::default() });
    let text: Vec<String> = layout.pages.iter().flatten().map(|l| l.plain()).collect();
    assert_eq!(text[0], "Budget 2026", "the document's own heading comes first");
    assert!(text.iter().any(|l| l == "Rent is 90.5 % of the expenses, the gross total is 1349.34 €."), "{text:#?}");
    assert!(text.iter().any(|l| l == "¹ Rent increase announced for July"));
    // Order follows the file: Expenses, its chart, the text, then Sales.
    let pos = |s: &str| text.iter().position(|l| l.starts_with(s)).unwrap();
    assert!(pos("Expenses") < pos("Rent is") && pos("Rent is") < pos("Sales"));
}

fn render_big(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 70)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mouse event at grid (column, row) of the layout editor.
fn grid_mouse(a: &mut App, kind: MouseEventKind, col: usize, row: usize) {
    use hardcalc::layout::{Element, ElementKind};
    let g = a.layout_geom.expect("layout drawn");
    let spec = a.raster_spec(0);
    let e = Element { cols: (col, col), rows: (row, row), kind: ElementKind::Text, content: String::new(), columns: vec![], label: None, z: None };
    let (x, _) = spec.span(g.cols, &e);
    let y = g.area.y + (g.line_of(row) - g.scroll) as u16;
    a.on_mouse(MouseEvent { kind, column: g.x0 + x as u16, row: y, modifiers: KeyModifiers::NONE });
}

fn element(a: &App, n: usize) -> hardcalc::layout::Element {
    a.raster_spec(0).elements[n].clone()
}

#[test]
fn layout_editor_keyboard() {
    use hardcalc::layout::ElementKind;
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    render(&mut a);
    click(&mut a, Hit::ViewTab(ViewKind::Layout(0)));
    assert_eq!(a.view.kind, ViewKind::Layout(0));
    assert_eq!(a.fkeys()[5].1, "Kind");
    assert!(render(&mut a).contains("Budget 2026"));

    // Arrows jump between elements by position.
    key(&mut a, KeyCode::Down); // heading → table
    assert_eq!(a.cursor_column_info(), "table · columns 1–6 · rows 4–12");
    key(&mut a, KeyCode::Right);
    assert_eq!(a.view.cursor.0, 2, "table → chart");
    key(&mut a, KeyCode::Down);
    assert_eq!(a.view.cursor.0, 4, "chart → value below it");
    key(&mut a, KeyCode::Left);
    assert_eq!(a.view.cursor.0, 3, "value → text");
    key(&mut a, KeyCode::Down);
    assert_eq!(a.view.cursor.0, 5, "text → note");
    key(&mut a, KeyCode::Down);
    assert_eq!(a.view.cursor.0, 5, "nothing below: stays");
    a.view.cursor.0 = 1;

    // Alt+arrows move (also ESC f from macOS Terminal).
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(element(&a, 1).cols, (2, 7));
    a.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT));
    assert_eq!(element(&a, 1).cols, (3, 8));
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert_eq!((element(&a, 1).cols, element(&a, 1).rows), ((1, 6), (4, 12)));
    // Without the move mode Shift+arrows only select, they do not resize.
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(a.view.cursor.0, 2);
    assert_eq!(element(&a, 1).rows, (4, 12));
    a.view.cursor.0 = 1;

    // F6 changes the kind: table → chart (content becomes a chart name).
    fkey(&mut a, 6);
    assert_eq!((element(&a, 1).kind, element(&a, 1).content.as_str()), (ElementKind::Chart, "Spending"));
    ctrl(&mut a, 'z');

    // Typing replaces the content of the selected element.
    key(&mut a, KeyCode::Up);
    typ(&mut a, "Jahresbericht");
    key(&mut a, KeyCode::Enter);
    assert_eq!(element(&a, 0).content, "Jahresbericht");
    assert_eq!(a.view.cursor.0, 0, "the selection stays");

    // F7 opens the paper menu (A4 portrait is marked); Down + Enter picks A4 landscape. F8 deletes.
    fkey(&mut a, 7);
    assert_eq!(a.paper_menu, Some(0));
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert!(a.paper_menu.is_none());
    assert!(a.layout_blocks()[0].1.contains(&"page: A4 landscape".to_string()));
    let n = a.raster_spec(0).elements.len();
    fkey(&mut a, 8);
    assert_eq!(a.raster_spec(0).elements.len(), n - 1);
}

#[test]
fn layout_editor_mouse() {
    use hardcalc::layout::ElementKind;
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.show_helper(ViewKind::Layout(0));
    render_big(&mut a);
    // Drag the note (rows 18–19) down to row 21.
    grid_mouse(&mut a, MouseEventKind::Down(MouseButton::Left), 2, 18);
    assert_eq!(a.view.cursor.0, 5);
    grid_mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 2, 21);
    grid_mouse(&mut a, MouseEventKind::Up(MouseButton::Left), 2, 21);
    assert_eq!(element(&a, 5).rows, (21, 22));
    // Resize at its bottom-right corner.
    render_big(&mut a);
    grid_mouse(&mut a, MouseEventKind::Down(MouseButton::Left), 12, 22);
    grid_mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 6, 24);
    grid_mouse(&mut a, MouseEventKind::Up(MouseButton::Left), 6, 24);
    assert_eq!((element(&a, 5).cols, element(&a, 5).rows), ((1, 6), (21, 24)));
    // Drawing on empty paper creates a text element and starts editing it.
    render_big(&mut a);
    grid_mouse(&mut a, MouseEventKind::Down(MouseButton::Left), 8, 30);
    grid_mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 11, 32);
    grid_mouse(&mut a, MouseEventKind::Up(MouseButton::Left), 11, 32);
    assert!(matches!(a.mode, Mode::Edit { .. }));
    typ(&mut a, "Hello");
    key(&mut a, KeyCode::Enter);
    let e = element(&a, 6);
    assert_eq!((e.kind, e.cols, e.rows, e.content.as_str()), (ElementKind::Text, (8, 11), (30, 32), "Hello"));
}

#[test]
fn layout_page_breaks_and_printing() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.show_helper(ViewKind::Layout(0));
    let body = hardcalc::print::layout_rows_per_page(&a.raster_spec(0).setup(0));
    // Move the note across the first page break.
    a.view.cursor.0 = 5;
    for _ in 18..body {
        a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    }
    let e = element(&a, 5);
    assert_eq!(e.rows, (body, body + 1));
    assert!(a.cursor_problem().is_some_and(|p| p.contains("page break")));
    let screen = render_big(&mut a);
    assert!(screen.contains("page 2"), "{screen}");

    // :print previews this layout; F5 there switches its orientation in the block.
    command(&mut a, "print");
    assert_eq!(a.print_setup.scope, hardcalc::print::Scope::Layout(0));
    assert!(render(&mut a).contains("layout Report"));
    fkey(&mut a, 5);
    assert!(a.layout_blocks()[0].1.contains(&"page: A4 landscape".to_string()));
    let pages = a.preview.as_ref().unwrap().layout.pages.len();
    assert!(pages >= 1);
}

#[test]
fn create_layout_command() {
    use hardcalc::layout::ElementKind;
    let mut a = app();
    typ(&mut a, ":layout Overview");
    key(&mut a, KeyCode::Enter);
    let i = a.layout_blocks().iter().position(|(n, _)| n == "Overview").unwrap();
    let spec = a.raster_spec(i);
    assert_eq!(spec.elements[0].kind, ElementKind::Heading);
    // Expenses has a chart: table left, chart right.
    assert_eq!((spec.elements[1].kind, spec.elements[1].cols), (ElementKind::Table, (1, 6)));
    assert_eq!((spec.elements[2].kind, spec.elements[2].cols), (ElementKind::Chart, (7, 12)));
    assert!(matches!(a.view.kind, hardcalc::tui::app::ViewKind::Layout(_)));
}

#[test]
fn tab_cycles_main_tabs() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    let mut seen = Vec::new();
    for _ in 0..6 {
        key(&mut a, KeyCode::Tab);
        seen.push((a.view.kind, a.view.sheet));
    }
    assert_eq!(
        seen,
        [
            (ViewKind::Table, 1),
            (ViewKind::Table, 2),
            (ViewKind::Table, 3),
            (ViewKind::Types, 3),
            (ViewKind::Functions, 3),
            (ViewKind::Layout(0), 3),
        ]
    );
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.view.kind, ViewKind::Table);
    a.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(a.view.kind, ViewKind::Layout(0));
}

#[test]
fn tab_rotates_pivot_dimensions() {
    let mut a = app();
    a.switch_sheet(1);
    ctrl(&mut a, 'p');
    let axes = |a: &App| {
        let p = a.view.pivot.as_ref().unwrap();
        (a.axis_name(p.row_axis), a.axis_name(p.col_axis))
    };
    assert_eq!(axes(&a), ("Month".into(), "Region".into()));
    key(&mut a, KeyCode::Tab);
    assert_eq!(axes(&a), ("Region".into(), "Product".into()));
    assert_eq!(a.dims(), (2, 2));
    // Product=A is no longer a filter: Month is now the fixed axis.
    let screen = render(&mut a);
    assert!(screen.contains("Month = Jan"), "{screen}");
    key(&mut a, KeyCode::Tab);
    assert_eq!(axes(&a), ("Product".into(), "Month".into()));
    key(&mut a, KeyCode::Tab);
    assert_eq!(axes(&a), ("Month".into(), "Region".into()));
    a.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(axes(&a), ("Product".into(), "Month".into()));
}

#[test]
fn paper_menu() {
    use hardcalc::print::Paper;
    let mut a = app();
    command(&mut a, "print");
    fkey(&mut a, 6);
    let screen = render(&mut a);
    for label in ["A3 portrait", "Legal landscape", "Ledger (Tabloid landscape)", "297×420 mm"] {
        assert!(screen.contains(label), "missing {label}\n{screen}");
    }
    // Click "Ledger" (the last entry).
    click(&mut a, Hit::PaperChoice(9));
    assert_eq!((a.print_setup.paper, a.print_setup.landscape), (Paper::Tabloid, true));
    let l = &a.preview.as_ref().unwrap().layout;
    assert_eq!((l.width, l.height), (1224.0, 792.0));
    // Esc closes the menu without changing anything.
    fkey(&mut a, 6);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Esc);
    assert!(a.paper_menu.is_none() && a.preview.is_some());
    assert_eq!(a.print_setup.paper, Paper::Tabloid);

    // Bigger paper, more room: A3 has more lines per page than A4.
    let a4 = hardcalc::print::page_grid(&hardcalc::print::PageSetup::default(), 9.0);
    let a3 = hardcalc::print::page_grid(&hardcalc::print::PageSetup { paper: Paper::A3, ..Default::default() }, 9.0);
    assert!(a3.0 > a4.0 && a3.1 > a4.1);
}

#[test]
fn image_preview_mode() {
    use hardcalc::tui::graphics::Protocol;
    let mut a = app();
    command(&mut a, "print");
    // Without an image protocol `i` shows a block-graphics overview of the page
    // (if a rasterizer is installed; otherwise it says so and stays with text).
    key(&mut a, KeyCode::Char('i'));
    let screen = render_big(&mut a);
    if a.image_mode {
        assert!(screen.matches('▀').count() > 500, "{screen}");
        assert!(screen.contains("overview (no image protocol)"));
    } else {
        assert!(a.message.as_ref().is_some_and(|m| m.text.contains("no PDF rasterizer")));
    }
    a.image_mode = false;

    a.graphics = Some(Protocol::Kitty);
    key(&mut a, KeyCode::Char('i'));
    assert!(a.image_mode);
    let screen = render_big(&mut a);
    assert!(screen.contains("i: text view"), "{screen}");
    let slot = a.image_slot.expect("image area reserved");
    // A4 portrait: about 0.71 wide per high, cells twice as high as wide.
    let ratio = slot.width as f32 / (2.0 * slot.height as f32);
    assert!((ratio - 0.707).abs() < 0.05, "{slot:?}");

    let pdf = a.preview_page_pdf().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/Count 1"));

    // Popups hide the image (it would cover them).
    fkey(&mut a, 6);
    render_big(&mut a);
    assert!(a.image_slot.is_none());
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char('i'));
    render_big(&mut a);
    assert!(a.image_slot.is_none() && !a.image_mode);
}

#[test]
fn layer_switches_global_or_per_table() {
    let mut a = app();
    let k = a.wb.sheet_index("Metrics").unwrap();
    let italic = |a: &mut App, s: usize| a.with_window(true, |a| {
        a.switch_sheet(s);
        a.cell_view(0, if s == 0 { 3 } else { 1 }).style.italic
    });
    // Default: all tables. Hiding "formulas" in Expenses hides it everywhere.
    let screen = render(&mut a);
    assert!(screen.contains("⊕ all tables"), "{screen}");
    a.toggle_layer("formulas");
    assert!(!italic(&mut a, 0) && !italic(&mut a, k));
    a.toggle_layer("formulas");
    assert!(italic(&mut a, 0) && italic(&mut a, k));

    // This table only: Expenses loses the marker, Metrics keeps it.
    a.switch_sheet(0);
    render(&mut a);
    click(&mut a, Hit::LayerScope);
    assert!(!a.layers_global);
    assert!(render(&mut a).contains("⊙ this table"));
    a.toggle_layer("formulas");
    assert!(a.layer_is_local("formulas"));
    assert!(render(&mut a).contains("□ formulas*"));
    assert!(!italic(&mut a, 0) && italic(&mut a, k));

    // Validation per table: hiding it in Expenses drops its violations only.
    a.switch_sheet(0);
    a.set_cells(vec![(0, 1, "-5".into())]);
    assert!(a.violations.contains_key(&(0, 0, 1)));
    a.toggle_layer("validate");
    assert!(!a.violations.contains_key(&(0, 0, 1)));

    // Switching back to all tables and toggling clears the exceptions.
    click(&mut a, Hit::LayerScope);
    a.switch_sheet(0);
    a.toggle_layer("formulas"); // hidden here, so this shows it in all tables
    assert!(!a.layer_is_local("formulas"));
    assert!(italic(&mut a, 0) && italic(&mut a, k));
}

fn right_click(app: &mut App, target: Hit) {
    let (rect, _) = *app.hits.iter().find(|(_, h)| *h == target).unwrap_or_else(|| panic!("no hit {target:?}"));
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
}

#[test]
fn exclusive_layer_view() {
    let mut a = app();
    render(&mut a);
    // Right click on "format" (index 1) shows the format layer exclusively.
    right_click(&mut a, Hit::Layer(1));
    assert_eq!(a.solo.as_deref(), Some("format"));
    assert!(render(&mut a).contains("◉ format"));
    assert_eq!(a.cell_view(1, 1).text, "align right, fg yellow");
    let plain = a.cell_view(0, 0);
    assert_eq!((plain.text.as_str(), plain.style.fg.clone()), ("Rent", Some(hardcalc::layers::Color::Named("darkgray".into()))));
    // A second right click (or Esc) returns to the normal view.
    right_click(&mut a, Hit::Layer(1));
    assert!(a.solo.is_none());
    right_click(&mut a, Hit::Layer(0));
    assert_eq!(a.cell_view(0, 3).text, "=B1/sum(B)");
    assert_eq!(a.cell_view(0, 1).text, "Euro");
    assert_eq!(a.cell_view(0, 0).text, "auto → text");
    key(&mut a, KeyCode::Esc);
    assert!(a.solo.is_none());
}

#[test]
fn editing_rules_in_the_layer_view() {
    let mut a = app();
    typ(&mut a, ":solo format");
    key(&mut a, KeyCode::Enter);
    // Typing on B3 creates the rule `B3: fg red`.
    a.view.cursor = (2, 1);
    assert_eq!(a.cursor_raw(), "");
    typ(&mut a, "fg red bold");
    key(&mut a, KeyCode::Enter);
    assert!(hardcalc::write(&a.doc).contains("B3: fg red bold"));
    assert_eq!(a.cell_view(2, 1).text, "align right, fg red, bold");
    // Editing shows the existing rule; Del removes it; undo restores it.
    a.view.cursor = (2, 1);
    assert_eq!(a.cursor_raw(), "fg red bold");
    key(&mut a, KeyCode::Delete);
    assert!(!hardcalc::write(&a.doc).contains("B3: fg red"));
    ctrl(&mut a, 'z');
    assert!(hardcalc::write(&a.doc).contains("B3: fg red bold"));

    // A selected range becomes a range rule.
    typ(&mut a, ":solo validate");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (0, 4);
    a.view.anchor = Some((2, 4));
    typ(&mut a, "<= 2000");
    key(&mut a, KeyCode::Enter);
    assert!(hardcalc::write(&a.doc).contains("E1:E3: <= 2000"));

    // A table without a comments layer gets one, right after the table.
    let k = a.wb.sheet_index("Metrics").unwrap();
    a.switch_sheet(k);
    typ(&mut a, ":solo comments");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (0, 1);
    typ(&mut a, "checked");
    key(&mut a, KeyCode::Enter);
    let src = hardcalc::write(&a.doc);
    let after_table = src.split("=spark(Expenses!Gross)").nth(1).unwrap();
    assert!(after_table.split_once("|\n\n").unwrap().1.starts_with("```layer comments\nB1: checked\n```"), "{src}");
    key(&mut a, KeyCode::Esc);
    a.view.cursor = (0, 1);
    assert_eq!(a.cursor_comment().as_deref(), Some("checked"));
}

#[test]
fn layer_view_does_not_change_printing_or_formula_editing() {
    let mut a = app();
    a.set_solo(Some("comments".into()));
    let items = a.print_items(false);
    let hardcalc::print::PrintItem::Table(t) = &items[0] else { panic!() };
    assert_eq!(t.rows[1][1].text, "85.50");
    // In the formulas view, typing edits the cell itself.
    a.set_solo(Some("formulas".into()));
    a.view.cursor = (1, 1);
    typ(&mut a, "90");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[0].raw[1][1], "90");
}

#[test]
fn light_terminal_theme() {
    use ratatui::style::Color;
    let draw = |a: &mut App| {
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| ui::draw(f, a)).unwrap();
        term.backend().buffer().clone()
    };
    let mut a = app();
    // Column header "Amount" (row 3, not the cursor's column) is gray: light gray on dark terminals …
    let dark = draw(&mut a);
    let x = (0..100).find(|&x| dark[(x, 3)].symbol() == "A").unwrap();
    assert_eq!(dark[(x, 3)].fg, Color::Gray);
    // … darker on light ones, whether detected or chosen.
    a.detected_light = Some(true);
    assert_eq!(draw(&mut a)[(x, 3)].fg, Color::Indexed(240));
    typ(&mut a, ":theme dark");
    key(&mut a, KeyCode::Enter);
    assert_eq!(draw(&mut a)[(x, 3)].fg, Color::Gray);
    // Chosen explicitly, the theme also paints the sheet: dark on a light terminal.
    let cell = draw(&mut a)[(5, 6)].clone();
    assert_eq!(cell.bg, hardcalc::tui::theme::DARK_PAPER);
    typ(&mut a, ":theme auto");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().unwrap().text.contains("terminal: light"));
    // Auto keeps the terminal's own background for the sheet.
    assert_eq!(draw(&mut a)[(5, 6)].bg, Color::Reset);
}

#[test]
fn functions_tab() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    render(&mut a);
    click(&mut a, Hit::ViewTab(ViewKind::Functions));
    assert_eq!(a.view.kind, ViewKind::Functions);
    let screen = render_big(&mut a);
    for s in ["ƒ functions", "Parameters", "gross", "x * 1.19", "gross amount with 19 % VAT", "part, total", "round", "values …"] {
        assert!(screen.contains(s), "missing {s:?}\n{screen}");
    }

    // A new function via the + row: name, then parameters (Tab), then definition.
    a.view.cursor = (2, 0);
    typ(&mut a, "Skonto");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view.cursor, (2, 1), "continues with the parameters");
    typ(&mut a, "betrag, tage");
    key(&mut a, KeyCode::Tab);
    typ(&mut a, "if(tage <= 10, betrag * 0.98, betrag)");
    key(&mut a, KeyCode::Enter);
    assert!(hardcalc::write(&a.doc).contains("skonto(betrag, tage) = if(tage <= 10, betrag * 0.98, betrag)"));

    // Used in a table, it computes – and the overview lists where.
    a.switch_sheet(0);
    a.set_cells(vec![(0, 5, "=skonto(Amount, 7)".into())]);
    assert!(matches!(a.wb.value((0, 0, 5)), hardcalc::value::Value::Num(n, _) if (n - 1176.0).abs() < 1e-9));
    click(&mut a, Hit::ViewTab(ViewKind::Functions));
    a.view.cursor = (2, 4);
    assert_eq!(a.cell_view(2, 4).text, "Expenses!F1");

    // Names of built-ins and invalid names are rejected; built-ins are read-only.
    a.view.cursor = (3, 0);
    typ(&mut a, "sum");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().is_some_and(|m| m.error));
    a.view.cursor = (6, 2);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.mode, Mode::Normal);

    // A broken definition is flagged; F8 deletes, undo restores.
    a.view.cursor = (0, 2);
    key(&mut a, KeyCode::Enter);
    typ(&mut a, " +");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (0, 2);
    assert!(a.cell_view(0, 2).error && a.cursor_problem().is_some());
    fkey(&mut a, 8);
    assert!(!hardcalc::write(&a.doc).contains("gross(x)"));
    ctrl(&mut a, 'z');
    assert!(hardcalc::write(&a.doc).contains("gross(x) = x * 1.19 +"));
}

#[test]
fn key_diagnosis() {
    let mut a = app();
    typ(&mut a, ":keys");
    key(&mut a, KeyCode::Enter);
    a.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert!(a.message.as_ref().unwrap().text.contains("Up ALT") || a.message.as_ref().unwrap().text.contains("Up KeyModifiers(ALT)"), "{:?}", a.message);
    typ(&mut a, ":keys");
    key(&mut a, KeyCode::Enter);
    assert!(!a.show_keys);
}

#[test]
fn meta_mode_alt_arrows_are_reassembled() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.show_helper(ViewKind::Layout(0));
    a.view.cursor.0 = 5; // the note, rows 18–19
    // ESC ESC [ B arrives as Esc, '[', 'B'.
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char('['));
    key(&mut a, KeyCode::Char('B'));
    assert_eq!(a.mode, Mode::Normal, "no text input started");
    assert_eq!(element(&a, 5).rows, (19, 20), "moved down");
    // A real '[' typed later still starts editing.
    std::thread::sleep(std::time::Duration::from_millis(30));
    key(&mut a, KeyCode::Char('['));
    assert!(matches!(a.mode, Mode::Edit { .. }));
}

#[test]
fn layout_move_mode() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.show_helper(ViewKind::Layout(0));
    assert_eq!(a.fkeys()[2].1, "Move");
    a.view.cursor.0 = 1; // table, columns 1–6, rows 4–12
    fkey(&mut a, 3);
    assert!(a.layout_grab);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!((element(&a, 1).cols, element(&a, 1).rows), ((2, 8), (5, 13)));
    assert_eq!(a.view.cursor.0, 1, "arrows move, they do not select");
    key(&mut a, KeyCode::Enter);
    assert!(!a.layout_grab);
    assert_eq!(a.mode, Mode::Normal, "Enter puts down, it does not edit");
    // Put down, arrows select again.
    key(&mut a, KeyCode::Right);
    assert_eq!(a.view.cursor.0, 2);
    // Esc and F3 also put it down.
    fkey(&mut a, 3);
    key(&mut a, KeyCode::Esc);
    assert!(!a.layout_grab);
    fkey(&mut a, 3);
    fkey(&mut a, 3);
    assert!(!a.layout_grab);
}

#[test]
fn resizing_pushes_the_elements_below() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    a.show_helper(ViewKind::Layout(0));
    // heading 1-12/1–2, table 1-6/4–12 | chart 7-12/4–12,
    // text 1-8/14–16 | value 9-12/14–16, note 1-12/18–19
    let rows = |a: &App| (0..6).map(|n| element(a, n).rows).collect::<Vec<_>>();
    let start = rows(&a);
    a.view.cursor.0 = 1;
    fkey(&mut a, 3);
    key(&mut a, KeyCode::Char('+'));
    key(&mut a, KeyCode::Char('+'));
    // The text below the table moves, the note below the text too; the chart
    // beside and the value (not below the table) stay.
    assert_eq!(rows(&a), [(1, 2), (4, 14), (4, 12), (16, 18), (14, 16), (20, 21)]);
    // Shrinking back restores the layout.
    a.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
    key(&mut a, KeyCode::Char('-'));
    assert_eq!(rows(&a), start);
    // Shrinking further: the chart above the text keeps it in place.
    key(&mut a, KeyCode::Char('-'));
    key(&mut a, KeyCode::Char('-'));
    assert_eq!(rows(&a)[1], (4, 10));
    assert_eq!(rows(&a)[3..], start[3..]);
    // > widens; each step is one undo step.
    key(&mut a, KeyCode::Char('>'));
    assert_eq!(element(&a, 1).cols, (1, 7));
    ctrl(&mut a, 'z');
    assert_eq!(element(&a, 1).cols, (1, 6));
    assert!(a.layout_grab, "still picked up");
}

#[test]
fn reflow_cascades_through_the_grid() {
    use hardcalc::layout::{Element, ElementKind};
    let el = |c: (usize, usize), r: (usize, usize)| Element { cols: c, rows: r, kind: ElementKind::Text, content: String::new(), columns: vec![], label: None, z: None };
    // A column of three stacked elements on the left, one on the right.
    let mut a = App::new("x.hcmd".into(), hardcalc::parse("```layout L
```
").unwrap());
    let lines: Vec<String> = [el((1, 4), (1, 3)), el((1, 4), (5, 6)), el((1, 4), (8, 9)), el((5, 12), (5, 20))].iter().map(|e| e.to_line()).collect();
    a.doc.blocks[0] = hardcalc::model::Block::Layout { name: "L".into(), lines };
    a.set_view(hardcalc::tui::app::ViewKind::Layout(0), 0);
    a.view.cursor.0 = 0;
    fkey(&mut a, 3);
    for _ in 0..3 {
        key(&mut a, KeyCode::Char('+'));
    }
    let rows: Vec<(usize, usize)> = a.raster_spec(0).elements.iter().map(|e| e.rows).collect();
    assert_eq!(rows, [(1, 6), (8, 9), (11, 12), (5, 20)], "the stack slides, the right column stays");
}

#[test]
fn help_has_pages() {
    use hardcalc::tui::app::ViewKind;
    let mut a = app();
    fkey(&mut a, 1);
    assert!(a.show_help && a.help_page == 0);
    assert!(render(&mut a).contains("1/7 · Basics"));
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Char(' '));
    assert_eq!(a.help_page, 2);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.help_page, 1);
    key(&mut a, KeyCode::Char('5'));
    let screen = render_big(&mut a);
    assert!(screen.contains("5/7 · Menu & commands") && screen.contains("F9, click on ≡ hardcalc"), "{screen}");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert_eq!(a.help_page, 0, "wraps around");
    // Page titles are clickable; a click elsewhere closes.
    render_big(&mut a);
    click(&mut a, Hit::HelpPage(3));
    assert_eq!(a.help_page, 3);
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
    assert!(!a.show_help);
    // Other keys do not close it; Esc does. Context: the layout editor opens page 4.
    a.show_helper(ViewKind::Layout(0));
    fkey(&mut a, 1);
    assert_eq!(a.help_page, 3);
    key(&mut a, KeyCode::Char('x'));
    assert!(a.show_help);
    key(&mut a, KeyCode::Esc);
    assert!(!a.show_help);
}

#[test]
fn header_navigation_selects_whole_lines() {
    let mut a = app();
    render(&mut a);
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Up);
    assert_eq!(a.on_header(), Some(Header::Column));
    assert_eq!(a.selection(), ((0, 1), (2, 1)));
    assert_eq!(a.cursor_label(), "B:B");
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(a.selection(), ((0, 1), (2, 2)));
    assert_eq!(a.cursor_label(), "B:C");
    // Typing edits the column name (Esc cancels); ↓ goes back into the column.
    typ(&mut a, "x");
    assert!(matches!(a.mode, Mode::Edit { .. }));
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Down);
    assert_eq!((a.on_header(), a.view.cursor, a.has_range()), (None, (0, 2), false));

    a.view.cursor = (1, 0);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.on_header(), Some(Header::Row));
    assert_eq!(a.selection(), ((1, 0), (1, 4)));
    assert_eq!(a.cursor_label(), "2:2");
    key(&mut a, KeyCode::Down);
    assert_eq!(a.selection(), ((2, 0), (2, 4)));
    key(&mut a, KeyCode::Right);
    assert_eq!((a.on_header(), a.view.cursor), (None, (2, 0)));

    // Mouse: a click on a row number selects the row.
    render(&mut a);
    click(&mut a, Hit::RowHeader(1));
    assert_eq!((a.on_header(), a.selection()), (Some(Header::Row), ((1, 0), (1, 4))));
    click(&mut a, Hit::ColHeader(3));
    assert_eq!((a.on_header(), a.selection()), (Some(Header::Column), ((0, 3), (2, 3))));
    click(&mut a, Hit::Cell(0, 0));
    assert_eq!(a.on_header(), None);
}

#[test]
fn minus_plus_move_rows_and_columns_with_their_references() {
    let mut a = app();
    let col0 = |a: &App| a.wb.sheets[0].raw.iter().map(|r| r[0].clone()).collect::<Vec<_>>();
    let share = |a: &mut App, r: usize| a.wb.value((0, r, 3)).display(None, false);
    let before = share(&mut a, 0);

    // Row 1 (Rent) down: its formula =B1/sum(B) becomes =B2/sum(B), the comment on B1 moves to B2.
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Left);
    typ(&mut a, "+");
    assert_eq!(col0(&a), ["Power", "Rent", "Internet"]);
    assert_eq!(a.view.cursor.0, 1);
    assert_eq!(a.wb.sheets[0].raw[1][3], "=B2/sum(B)");
    assert_eq!(a.wb.sheets[0].raw[0][3], "=B1/sum(B)");
    assert_eq!(share(&mut a, 1), before);
    assert!(hardcalc::write(&a.doc).contains("B2: Rent increase"));
    typ(&mut a, "++"); // at the end: nothing happens
    assert_eq!(col0(&a), ["Power", "Internet", "Rent"]);
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert_eq!(col0(&a), ["Rent", "Power", "Internet"]);

    // Column Amount (B) right: references to B become C.
    key(&mut a, KeyCode::Esc); // back into the cells
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Up);
    // Alt+arrows do not move (unreliable in terminals).
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    key(&mut a, KeyCode::Left);
    typ(&mut a, "+");
    let names: Vec<_> = a.wb.sheets[0].columns.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["Item", "Status", "Amount", "Share", "Gross"]);
    assert_eq!(a.wb.sheets[0].raw[0][3], "=C1/sum(C)");
    assert_eq!(share(&mut a, 0), before);
    assert!(hardcalc::write(&a.doc).contains("C1: Rent increase"));
    assert_eq!(a.view.cursor.1, 2);

    typ(&mut a, "-");
    assert_eq!(a.wb.sheets[0].columns[1].name, "Amount");
    typ(&mut a, "+");
    assert_eq!(a.wb.sheets[0].columns[2].name, "Amount");

    // F8 on the column header deletes the column.
    fkey(&mut a, 8);
    assert_eq!(a.wb.sheets[0].columns.len(), 4);
}

#[test]
fn sort_keeps_references_with_their_rows() {
    let mut a = app();
    a.view.cursor = (0, 1);
    typ(&mut a, ":sort");
    key(&mut a, KeyCode::Enter);
    // Internet, Power, Rent: each share still uses its own amount.
    assert_eq!(a.wb.sheets[0].raw[2][3], "=B3/sum(B)");
    assert_eq!(a.wb.sheets[0].raw[0][3], "=B1/sum(B)");
    assert!(hardcalc::write(&a.doc).contains("B3: Rent increase"));
}

#[test]
fn table_origin_renames_the_table() {
    let mut a = app();
    render(&mut a);
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.on_header(), Some(Header::Corner));
    assert_eq!(a.selection(), ((0, 0), (2, 4)));
    assert_eq!(a.cursor_raw(), "Expenses");
    // Back: → to the column headers, ↓ to the row headers.
    key(&mut a, KeyCode::Down);
    assert_eq!(a.on_header(), Some(Header::Row));
    key(&mut a, KeyCode::Up);
    assert_eq!(a.on_header(), Some(Header::Corner));

    typ(&mut a, "Costs");
    let screen = render(&mut a);
    // Edited in the formula bar; A1 keeps showing its value.
    assert!(screen.contains("Costs") && screen.contains("Rent"), "{screen}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.on_header(), Some(Header::Corner));
    assert_eq!(a.wb.sheets[0].name.as_deref(), Some("Costs"));
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("## Costs :: matrix"));
    assert!(text.contains("data:   Costs.Amount") || text.contains("data: Costs.Amount"), "{text}");
    assert!(!text.contains("Expenses."), "{text}");

    typ(&mut a, "a.b");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().unwrap().error);

    // Names with spaces: references are quoted.
    key(&mut a, KeyCode::Enter);
    for _ in 0..10 {
        key(&mut a, KeyCode::Backspace);
    }
    typ(&mut a, "Meine  Costs ");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[0].name.as_deref(), Some("Meine Costs"));
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("## Meine Costs :: matrix"), "{text}");
    assert!(text.contains("Meine Costs.Amount"), "{text}");
    assert!(a.chart_data(0).is_ok(), "{:?}", a.chart_data(0).err());
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert_eq!(a.wb.sheets[0].name.as_deref(), Some("Expenses"));

    render(&mut a);
    click(&mut a, Hit::Cell(1, 1));
    click(&mut a, Hit::Corner);
    assert_eq!(a.on_header(), Some(Header::Corner));
}

#[test]
fn banded_rows_and_columns() {
    use ratatui::style::Color;
    let mut a = app();
    a.view.cursor = (2, 4); // keep the cursor away from the cells checked
    let bg = |a: &mut App, r: usize, c: usize| {
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| ui::draw(f, a)).unwrap();
        let (rect, _) = *a.hits.iter().find(|(_, h)| *h == Hit::Cell(r, c)).unwrap();
        term.backend().buffer()[(rect.x, rect.y)].bg
    };
    let shades = [bg(&mut a, 0, 0), bg(&mut a, 0, 1), bg(&mut a, 1, 0), bg(&mut a, 1, 1)];
    assert_eq!(shades[0], Color::Reset);
    assert!(shades[1] != Color::Reset && shades[2] != Color::Reset && shades[3] != shades[1], "{shades:?}");
    // The placeholder row stays plain.
    assert_eq!(bg(&mut a, 3, 1), Color::Reset);
    typ(&mut a, ":bands");
    key(&mut a, KeyCode::Enter);
    assert_eq!(bg(&mut a, 1, 1), Color::Reset);
}

#[test]
fn playground_fills_the_window_and_saves_the_used_area() {
    let mut a = app();
    typ(&mut a, ":playground Notizen");
    key(&mut a, KeyCode::Enter);
    assert!(a.fills());
    let screen = render(&mut a);
    // Columns A, B, C … and row numbers right away.
    assert!(screen.contains(" D ") || screen.contains("D  "), "{screen}");
    assert!(screen.contains("  5"), "{screen}");
    a.hits.iter().find(|(_, h)| *h == Hit::Cell(8, 5)).expect("cells to the edge");

    // Typing far out grows the table up to there.
    a.view.cursor = (4, 3);
    typ(&mut a, "42");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (1, 1);
    typ(&mut a, "=D5*2");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.value((a.view.sheet, 1, 1)).display(None, false), "84");
    // Clearing the far cell: the formula's row and column remain, the rest is not written.
    a.view.cursor = (4, 3);
    key(&mut a, KeyCode::Delete);
    let text = hardcalc::write(&a.doc);
    let part = &text[text.find("## Notizen :: playground").unwrap()..];
    assert_eq!(part.lines().filter(|l| l.starts_with('|')).count(), 4, "header, separator, 2 rows: {part}");
    assert!(part.contains("=D5*2 |\n"), "{part}");
    assert!(!part.contains("| D"), "{part}");

    // :fill turns it off (and on for other tables).
    typ(&mut a, ":fill");
    key(&mut a, KeyCode::Enter);
    assert!(!a.fills());
    a.switch_sheet(0);
    assert!(!a.fills());
    typ(&mut a, ":fill on");
    key(&mut a, KeyCode::Enter);
    assert!(a.fills());

    // Both are kept in the headings, the default is not written.
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("## Notizen :: playground nofill\n"), "{text}");
    let reopened = App::new(PathBuf::from("budget.hcmd"), hardcalc::parse(&text).unwrap());
    assert!(reopened.fills(), "the first table fills again");
    a.switch_sheet(a.wb.sheets.len() - 1);
    typ(&mut a, ":fill on");
    key(&mut a, KeyCode::Enter);
    assert!(!hardcalc::write(&a.doc).contains("nofill"));
}

#[test]
fn fill_flag_in_the_heading() {
    let src = "## Account :: matrix fill [January]\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n## P :: playground nofill\n\n| A |\n|---|\n| 1 |\n";
    let doc = hardcalc::parse(src).unwrap();
    assert_eq!(hardcalc::write(&doc), src);
    let mut a = App::new(PathBuf::from("x.hcmd"), doc);
    assert!(a.fills());
    a.switch_sheet(1);
    assert!(!a.fills());
}

#[test]
fn column_header_names_the_column() {
    let mut a = app();
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Up);
    assert_eq!(a.cursor_raw(), "Amount:Euro");
    key(&mut a, KeyCode::Enter);
    for _ in 0.."Amount:Euro".len() {
        key(&mut a, KeyCode::Backspace);
    }
    typ(&mut a, "Total:Euro");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.on_header(), Some(Header::Column), "stays on the header");
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("Total:Euro"), "{text}");
    assert!(text.contains("=Total * 1.19") && text.contains("Expenses.Total"), "{text}");
    assert!(!a.wb.value((0, 1, 4)).is_error());

    // Ambiguous or taken names are refused.
    for bad in ["C", "B2", "Item"] {
        key(&mut a, KeyCode::Enter);
        for _ in 0..12 {
            key(&mut a, KeyCode::Backspace);
        }
        typ(&mut a, bad);
        key(&mut a, KeyCode::Enter);
        assert!(a.message.as_ref().unwrap().error, "{bad}");
    }
}

#[test]
fn playground_headers_reach_the_window_edge() {
    let mut a = app();
    typ(&mut a, ":playground");
    key(&mut a, KeyCode::Enter);
    render(&mut a);
    // Keyboard: along the column headers into the empty area, and name one.
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert_eq!((a.on_header(), a.cursor_label().as_str()), (Some(Header::Column), "C:C"));
    typ(&mut a, "Price");
    key(&mut a, KeyCode::Enter);
    let names: Vec<_> = a.wb.sheets[a.view.sheet].columns.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["A", "B", "Price"]);
    // Row headers far down, by keyboard and mouse.
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left); // corner
    key(&mut a, KeyCode::Down);
    for _ in 0..6 {
        key(&mut a, KeyCode::Down);
    }
    assert_eq!((a.on_header(), a.cursor_label().as_str()), (Some(Header::Row), "7:7"));
    render(&mut a);
    click(&mut a, Hit::RowHeader(9));
    assert_eq!(a.cursor_label(), "10:10");
    click(&mut a, Hit::ColHeader(6));
    assert_eq!(a.cursor_label(), "G:G");
    // Moving a column into the empty area grows the table.
    click(&mut a, Hit::ColHeader(2));
    typ(&mut a, "+");
    let names: Vec<_> = a.wb.sheets[a.view.sheet].columns.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["A", "B", "C", "Price"], "letter names follow their position");
}

fn input(a: &App) -> String {
    match &a.mode {
        Mode::Edit { buf, .. } => buf.clone(),
        _ => panic!("not editing"),
    }
}

#[test]
fn arrows_pick_references_while_editing_a_formula() {
    let mut a = app();
    a.view.cursor = (2, 3); // Share of Internet
    typ(&mut a, "=");
    key(&mut a, KeyCode::Left);
    assert_eq!(input(&a), "=C3");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Up);
    assert_eq!(input(&a), "=B2", "the picked reference is replaced");
    assert_eq!(a.point_range(), Some(((1, 1), (1, 1))));
    // Shift extends to a range.
    a.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
    assert_eq!(input(&a), "=B1:B2");
    // Typing an operator ends it; the next arrow starts a new reference.
    typ(&mut a, "/sum(");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    assert_eq!(input(&a), "=B1:B2/sum(B3");
    typ(&mut a, ")");
    // After a complete operand, arrows commit and move as before.
    key(&mut a, KeyCode::Down);
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.wb.sheets[0].raw[2][3], "=B1:B2/sum(B3)");

    // Not a formula: arrows finish the input.
    a.view.cursor = (0, 0);
    typ(&mut a, "Haus");
    key(&mut a, KeyCode::Down);
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.view.cursor, (1, 0));

    // Mouse: click picks a cell, dragging a range.
    render(&mut a);
    typ(&mut a, "=sum(");
    let at = |a: &App, r, c| a.hits.iter().find(|(_, h)| *h == Hit::Cell(r, c)).unwrap().0;
    let (p, q) = (at(&a, 0, 1), at(&a, 2, 1));
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: p.x, row: p.y, modifiers: KeyModifiers::NONE });
    assert_eq!(input(&a), "=sum(B1");
    a.on_mouse(MouseEvent { kind: MouseEventKind::Drag(MouseButton::Left), column: q.x, row: q.y, modifiers: KeyModifiers::NONE });
    a.on_mouse(MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), column: q.x, row: q.y, modifiers: KeyModifiers::NONE });
    assert_eq!(input(&a), "=sum(B1:B3");
    assert_eq!(a.view.cursor, (1, 0), "the edited cell stays");
}

#[test]
fn colon_continues_a_picked_reference_into_a_range() {
    // Without Shift (Terminal.app does not send Shift+↑/↓): B1, ':', arrows.
    let mut a = app();
    a.view.cursor = (2, 3);
    typ(&mut a, "=sum(");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    assert_eq!(input(&a), "=sum(B1");
    typ(&mut a, ":");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    assert_eq!(input(&a), "=sum(B1:B3");
    assert_eq!(a.point_range(), Some(((0, 1), (2, 1))), "the whole range is highlighted");
}

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hardcalc-tui-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn command(a: &mut App, cmd: &str) {
    typ(a, &format!(":{cmd}"));
    key(a, KeyCode::Enter);
}

#[test]
fn untitled_document_open_and_append() {
    let dir = scratch("files");
    let mut a = App::untitled();
    assert!(a.is_untitled() && a.fills(), "a playground to start with");
    typ(&mut a, "42");
    key(&mut a, KeyCode::Enter);

    // F2 asks for a name; .hcmd is added.
    fkey(&mut a, 2);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "w "));
    typ(&mut a, &dir.join("notiz").display().to_string());
    key(&mut a, KeyCode::Enter);
    let saved = dir.join("notiz.hcmd");
    assert!(std::fs::read_to_string(&saved).unwrap().contains("| 42"), "{:?}", a.message);
    assert!(!a.dirty);

    // :e opens another file; unsaved changes are guarded.
    std::fs::write(dir.join("budget.hcmd"), BUDGET).unwrap();
    typ(&mut a, "7");
    key(&mut a, KeyCode::Enter);
    command(&mut a, &format!("e {}", dir.join("budget.hcmd").display()));
    assert!(a.confirm.is_some(), "save or discard?");
    key(&mut a, KeyCode::Esc);
    assert!(a.confirm.is_none() && a.dirty, "cancelled");
    command(&mut a, &format!("e {}", dir.join("budget.hcmd").display()));
    typ(&mut a, "d");
    assert_eq!(a.wb.sheets[0].name.as_deref(), Some("Expenses"));
    // :e! skips the question.
    typ(&mut a, "7");
    key(&mut a, KeyCode::Enter);
    command(&mut a, &format!("e! {}", dir.join("budget.hcmd").display()));
    assert!(a.confirm.is_none() && !a.dirty);

    // :append adds the other document; a taken table name gets a number.
    command(&mut a, &format!("append {}", dir.join("budget.hcmd").display()));
    let names: Vec<_> = a.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
    assert!(names.contains(&"Expenses 2".to_string()), "{names:?}");
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("data: Expenses 2.Amount, Expenses 2.Gross"), "the copy's chart follows: {text}");
    assert_eq!(text.matches("# Budget 2026").count(), 1, "the title is not appended");
    assert!(a.dirty);

    // :new starts over.
    command(&mut a, "new!");
    assert!(a.is_untitled());
}

#[test]
fn tab_completes_file_names() {
    let dir = scratch("complete");
    std::fs::write(dir.join("kosten-2026.hcmd"), "").unwrap();
    std::fs::write(dir.join("kosten-2025.hcmd"), "").unwrap();
    std::fs::create_dir(dir.join("archiv")).unwrap();
    let mut a = App::untitled();
    let base = dir.display().to_string() + std::path::MAIN_SEPARATOR_STR;
    typ(&mut a, &format!(":e {base}ko"));
    key(&mut a, KeyCode::Tab);
    assert!(matches!(&a.mode, Mode::Command { buf } if *buf == format!("e {base}kosten-202")), "{:?}", a.mode);
    typ(&mut a, "6");
    key(&mut a, KeyCode::Tab);
    assert!(matches!(&a.mode, Mode::Command { buf } if *buf == format!("e {base}kosten-2026.hcmd")), "{:?}", a.mode);
    key(&mut a, KeyCode::Esc);
    typ(&mut a, &format!(":append {base}ar"));
    key(&mut a, KeyCode::Tab);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf.ends_with(&format!("archiv{}", std::path::MAIN_SEPARATOR))), "{:?}", a.mode);
}

#[test]
fn file_dialog_opens_and_appends() {
    let dir = scratch("dialog");
    std::fs::write(dir.join("budget.hcmd"), BUDGET).unwrap();
    std::fs::write(dir.join("zweit.hcmd"), "## Zweit :: matrix\n\n| A |\n|---|\n| 1 |\n").unwrap();
    std::fs::write(dir.join("notiz.txt"), "x").unwrap();
    std::fs::create_dir(dir.join("archiv")).unwrap();
    let mut a = App::open(&dir.join("budget.hcmd")).unwrap();
    let names = |a: &App| a.file_dialog.as_ref().unwrap().shown().iter().map(|e| e.name.clone()).collect::<Vec<_>>();

    ctrl(&mut a, 'e');
    assert_eq!(names(&a), ["..", "archiv", "budget.hcmd", "zweit.hcmd"], "folders first, documents only");
    let screen = render(&mut a);
    assert!(screen.contains("open file") && screen.contains("▸ archiv/"), "{screen}");
    ctrl(&mut a, 'a');
    assert!(names(&a).contains(&"notiz.txt".to_string()));
    ctrl(&mut a, 'a');

    // Into a folder and back up: the folder stays selected.
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert!(a.file_dialog.as_ref().unwrap().dir.ends_with("archiv"));
    key(&mut a, KeyCode::Left);
    assert_eq!(a.file_dialog.as_ref().unwrap().current().unwrap().name, "archiv");

    // Quick search, Enter opens.
    typ(&mut a, "zwe");
    assert_eq!(names(&a), ["zweit.hcmd"]);
    key(&mut a, KeyCode::Enter);
    assert!(a.file_dialog.is_none());
    assert_eq!(a.wb.sheets[0].name.as_deref(), Some("Zweit"));

    // :append without a file: the dialog for appending; a double click takes the file.
    command(&mut a, "append");
    assert_eq!(a.file_dialog.as_ref().unwrap().purpose, hardcalc::tui::app::Purpose::Append);
    render(&mut a);
    let i = names(&a).iter().position(|n| n == "budget.hcmd").unwrap();
    click(&mut a, Hit::FileEntry(i));
    click(&mut a, Hit::FileEntry(i));
    assert!(a.file_dialog.is_none());
    assert!(a.wb.sheets.iter().any(|s| s.name.as_deref() == Some("Expenses")));

    // Unsaved changes: the question comes before the dialog.
    ctrl(&mut a, 'e');
    assert!(a.confirm.is_some() && a.file_dialog.is_none());
    let screen = render(&mut a);
    assert!(screen.contains("unsaved changes") && screen.contains("[D]iscard"), "{screen}");
    click(&mut a, Hit::Confirm(1));
    assert!(a.file_dialog.is_some());
    key(&mut a, KeyCode::Esc);
    assert!(a.file_dialog.is_none());
}

#[test]
fn saving_a_new_document_before_opening_another() {
    let dir = scratch("save-first");
    let mut a = App::untitled();
    typ(&mut a, "1");
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'e');
    typ(&mut a, "s");
    // A new document asks for its name first; then the dialog opens.
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "w "));
    typ(&mut a, &dir.join("erst").display().to_string());
    key(&mut a, KeyCode::Enter);
    assert!(dir.join("erst.hcmd").exists() && !a.dirty);
    assert!(a.file_dialog.is_some(), "{:?}", a.message);
    key(&mut a, KeyCode::Esc);

    // Leaving the name prompt drops the waiting action.
    typ(&mut a, "2");
    key(&mut a, KeyCode::Enter);
    a = App::untitled();
    typ(&mut a, "3");
    key(&mut a, KeyCode::Enter);
    command(&mut a, "new");
    typ(&mut a, "s");
    key(&mut a, KeyCode::Esc);
    fkey(&mut a, 2);
    typ(&mut a, &dir.join("zweit").display().to_string());
    key(&mut a, KeyCode::Enter);
    assert!(a.file_dialog.is_none() && !a.is_untitled(), "no :new after a later save");
}


#[test]
fn menu_bar_runs_commands() {
    let mut a = app();
    // F9 opens the menu bar in place of the tabs.
    fkey(&mut a, 9);
    let screen = render(&mut a);
    assert!(screen.contains("File") && screen.contains("Insert") && screen.contains("Save as…"), "{screen}");
    assert!(!screen.contains("Metrics"), "the menu bar replaces the tabs: {screen}");
    // → to Edit, ↓ to Redo … Enter runs; Esc closes.
    key(&mut a, KeyCode::Right);
    assert_eq!(a.menu, Some((1, 0)));
    key(&mut a, KeyCode::Esc);
    assert!(a.menu.is_none());

    // View → Split window via keys; a letter jumps to the entry.
    fkey(&mut a, 9);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    typ(&mut a, "s"); // several entries: jumps to the next one (Swap windows)
    assert_eq!(a.menu, Some((2, 1)));
    typ(&mut a, "i"); // the only one: runs (Inspector)
    assert!(a.is_split() && a.menu.is_none());

    // Mouse: click on ≡ hardcalc, then on Insert → Playground.
    render(&mut a);
    click(&mut a, Hit::Logo);
    assert_eq!(a.menu, Some((0, 0)));
    render(&mut a);
    click(&mut a, Hit::MenuTop(3));
    assert_eq!(a.menu, Some((3, 0)));
    render(&mut a);
    click(&mut a, Hit::MenuItem(1));
    assert!(a.menu.is_none());
    assert!(a.fills(), "a new playground");

    // An entry needing an argument opens the command line with it.
    fkey(&mut a, 9);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "w "), "Save as…");
}

#[test]
fn command_line_suggests_and_completes() {
    let mut a = app();
    typ(&mut a, ":th");
    assert_eq!(a.command_suggestions()[0].0, "theme ");
    let screen = render(&mut a);
    assert!(screen.contains(":theme auto|light|dark") && screen.contains("colour scheme"), "{screen}");
    key(&mut a, KeyCode::Tab);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "theme "));
    typ(&mut a, "d");
    key(&mut a, KeyCode::Tab);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "theme dark"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.theme, hardcalc::tui::theme::ThemeSetting::Dark);

    // ↑↓ choose; Enter takes the suggestion (one needing an argument stays open).
    typ(&mut a, ":so");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "solo " || buf == "sort "), "{:?}", a.mode);
    // Dynamic values: layer names.
    key(&mut a, KeyCode::Esc);
    typ(&mut a, ":solo fo");
    let values: Vec<String> = a.command_suggestions().into_iter().map(|s| s.0).collect();
    assert!(values.contains(&"solo format".to_string()) && values.contains(&"solo formulas".to_string()), "{values:?}");
    key(&mut a, KeyCode::Esc);
    // A click on a suggestion completes too.
    typ(&mut a, ":vers");
    render(&mut a);
    click(&mut a, Hit::Suggestion(0));
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().unwrap().text.starts_with("hardcalc "));
}


#[test]
fn header_highlight_covers_the_shown_line_in_an_empty_playground() {
    use ratatui::style::Color;
    let mut a = App::untitled();
    render(&mut a);
    click(&mut a, Hit::ColHeader(2));
    let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
    term.draw(|f| ui::draw(f, &mut a)).unwrap();
    let bg = |r: usize, c: usize| {
        let (rect, _) = *a.hits.iter().find(|(_, h)| *h == Hit::Cell(r, c)).unwrap();
        term.backend().buffer()[(rect.x, rect.y)].bg
    };
    assert_eq!((bg(0, 2), bg(8, 2)), (Color::Blue, Color::Blue), "the whole shown column");
    assert_ne!(bg(8, 1), Color::Blue);
    a.view.header = None;
    click(&mut a, Hit::RowHeader(5));
    term.draw(|f| ui::draw(f, &mut a)).unwrap();
    let bg = |r: usize, c: usize| {
        let (rect, _) = *a.hits.iter().find(|(_, h)| *h == Hit::Cell(r, c)).unwrap();
        term.backend().buffer()[(rect.x, rect.y)].bg
    };
    assert_eq!((bg(5, 0), bg(5, 6)), (Color::Blue, Color::Blue), "the whole shown row");
}

#[test]
fn type_command_sets_the_column_type() {
    let mut a = App::untitled();
    typ(&mut a, "12");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "x");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (0, 0);
    command(&mut a, "type int");
    assert_eq!(hardcalc::model::Column { name: "A".into(), ty: Some("int".into()), align: hardcalc::model::Align::None }, a.doc_column(0, 0));
    assert!(a.message.as_ref().unwrap().error, "x is no int: {:?}", a.message);
    command(&mut a, "type");
    assert!(a.message.as_ref().unwrap().text.contains("type int"));
    command(&mut a, "type none");
    assert_eq!(a.doc_column(0, 0).ty, None);
    command(&mut a, "type nonsense(");
    assert!(a.message.as_ref().unwrap().error);
    // Beyond the data in a playground: the column is added.
    a.view.cursor = (0, 3);
    command(&mut a, "type decimal(2)");
    assert_eq!(a.doc_column(0, 3).ty.as_deref(), Some("decimal(2)"));
    // Suggestions offer types.
    typ(&mut a, ":type de");
    assert!(a.command_suggestions().iter().any(|s| s.0 == "type decimal(2)"));
}

#[test]
fn filling_copies_formulas_and_continues_series() {
    let mut a = App::untitled();
    let raw = |a: &App, r: usize, c: usize| a.wb.sheets[0].raw.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
    // A: 1, 2 · B: =A1*$C$1 · C1: 10
    for (r, c, t) in [(0, 0, "1"), (1, 0, "2"), (0, 1, "=A1*$C$1"), (0, 2, "10"), (0, 3, "Montag")] {
        a.view.cursor = (r, c);
        typ(&mut a, t);
        key(&mut a, KeyCode::Enter);
    }
    // :autofill continues the selection A1:A2 down to A5.
    a.view.cursor = (0, 0);
    a.view.anchor = Some((1, 0));
    command(&mut a, "autofill A5");
    assert_eq!((0..5).map(|r| raw(&a, r, 0)).collect::<Vec<_>>(), ["1", "2", "3", "4", "5"]);
    // Ctrl+D: B1 into B1:B5, the relative reference moves along, $C$1 stays.
    a.view.anchor = Some((0, 1));
    a.view.cursor = (4, 1);
    ctrl(&mut a, 'd');
    assert_eq!(raw(&a, 4, 1), "=A5*$C$1");
    assert_eq!(a.wb.value((0, 4, 1)).display(None, false), "50");
    // Ctrl+R with a single cell: from the left neighbour.
    a.view.anchor = None;
    a.view.cursor = (1, 2);
    ctrl(&mut a, 'r');
    assert_eq!(raw(&a, 1, 2), "=B2*$C$1");
    // The fill handle: drag from D1 (Montag) down to D3.
    a.view.anchor = None;
    a.view.cursor = (0, 3);
    render(&mut a);
    click(&mut a, Hit::FillHandle);
    let target = a.hits.iter().find(|(_, h)| *h == Hit::Cell(2, 3)).unwrap().0;
    let (hx, hy) = {
        let (r, _) = a.hits.iter().find(|(_, h)| *h == Hit::Cell(0, 3)).unwrap();
        (r.x + r.width, r.y)
    };
    for (kind, x, y) in [
        (MouseEventKind::Down(MouseButton::Left), hx, hy),
        (MouseEventKind::Drag(MouseButton::Left), target.x, target.y),
        (MouseEventKind::Up(MouseButton::Left), target.x, target.y),
    ] {
        a.on_mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
    }
    assert_eq!((raw(&a, 1, 3), raw(&a, 2, 3)), ("Dienstag".to_string(), "Mittwoch".to_string()));
    // Undo takes the whole fill back.
    ctrl(&mut a, 'z');
    assert_eq!(raw(&a, 2, 3), "");
}


#[test]
fn command_preview_like_helix() {
    let mut a = app();
    typ(&mut a, ":");
    let screen = render(&mut a);
    assert!(screen.contains("autofill") && screen.contains("playground"), "all commands as a grid: {screen}");
    assert!(!screen.contains("write (save)"), "no documentation before anything is typed");
    typ(&mut a, "open");
    let screen = render(&mut a);
    assert!(screen.contains(":e [file]") && screen.contains("also: :edit  :open"), "aliases are documented: {screen}");
    key(&mut a, KeyCode::Esc);
    typ(&mut a, ":fil");
    key(&mut a, KeyCode::Down); // the first: filldown
    let screen = render(&mut a);
    assert!(screen.contains(":filldown") && screen.contains("menu: Edit → Fill down (Ctrl+D)"), "the chosen one: {screen}");
}

#[test]
fn backspace_and_delete_in_a_fresh_document() {
    for code in [KeyCode::Backspace, KeyCode::Delete] {
        let mut a = App::untitled();
        render(&mut a);
        key(&mut a, code);
        render(&mut a);
        // Also on headers, the corner and far out in the playground.
        key(&mut a, KeyCode::Up);
        key(&mut a, code);
        key(&mut a, KeyCode::Left);
        key(&mut a, code);
        a.view.header = None;
        a.view.cursor = (7, 5);
        key(&mut a, code);
        render(&mut a);
        assert!(!a.dirty);
    }
}

#[test]
fn no_key_crashes_a_fresh_or_example_document() {
    let mut keys: Vec<KeyEvent> = [
        KeyCode::Backspace, KeyCode::Delete, KeyCode::Enter, KeyCode::Esc, KeyCode::Insert, KeyCode::Tab, KeyCode::BackTab,
        KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right, KeyCode::Home, KeyCode::End, KeyCode::PageUp, KeyCode::PageDown,
        KeyCode::Char('-'), KeyCode::Char('+'),
    ]
    .into_iter()
    .map(|c| KeyEvent::new(c, KeyModifiers::NONE))
    .collect();
    keys.extend((1..=9).map(|n| KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)));
    keys.extend("zyxcvpdrueo".chars().map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
    // F2 saves: the example goes to the temp folder, not into the project.
    fn example() -> App {
        App::new(std::env::temp_dir().join("hardcalc-no-key-crashes.hcmd"), hardcalc::parse(BUDGET).unwrap())
    }
    let starts: [fn() -> App; 2] = [App::untitled, example];
    for start in starts {
        for k in &keys {
            for (r, c) in [(0, 0), (5, 3)] {
                let mut a = start();
                a.view.cursor = (r, c);
                render(&mut a);
                a.on_key(*k);
                render(&mut a);
                // Leave whatever opened and try once more.
                a.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                a.on_key(*k);
                render(&mut a);
            }
        }
    }
}

#[test]
fn footer_rows() {
    let mut a = app();
    a.command("footer Total");
    assert_eq!(a.view.cursor.0, 4, "below the + row");
    a.view.cursor.1 = 1;
    typ(&mut a, "=sum");
    key(&mut a, KeyCode::Enter);
    assert!(hardcalc::write(&a.doc).contains("| Total | "), "{}", hardcalc::write(&a.doc));
    let screen = render(&mut a);
    assert!(screen.contains("Total") && screen.contains("1325.49"), "{screen}");
    a.view.cursor = (4, 2);
    assert_eq!(a.cursor_label(), "Status[Total]");

    // A count, and a text in the first column.
    typ(&mut a, "=counta(Status)");
    key(&mut a, KeyCode::Enter);
    assert!(render(&mut a).contains(" 3 "));
    a.view.cursor = (4, 0);
    typ(&mut a, "all items");
    key(&mut a, KeyCode::Enter);
    assert!(render(&mut a).contains("all items"));
    let footer = |a: &App| a.table().footer.iter().map(|f| (f.label.clone(), f.cells.clone())).collect::<Vec<_>>();
    assert_eq!(footer(&a)[0].1[..3], ["all items", "=sum", "=counta(Status)"]);

    // The label is renamed on the row header.
    a.view.cursor = (4, 0);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.on_header(), Some(Header::Row));
    typ(&mut a, "Total");
    key(&mut a, KeyCode::Enter);
    assert_eq!(footer(&a)[0].0, "Total");
    assert!(render(&mut a).contains("Total"));
    key(&mut a, KeyCode::Right);

    // Typing sum on the + row appends a data row, not a footer.
    a.view.cursor = (3, 1);
    typ(&mut a, "10");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[0].cells.len(), 4);
    assert!(render(&mut a).contains("1335.49"), "footer follows the new row");

    // Del clears a footer cell, F8 deletes the footer row, undo restores it.
    a.view.cursor = (5, 2);
    key(&mut a, KeyCode::Delete);
    assert_eq!(footer(&a)[0].1[2], "");
    fkey(&mut a, 8);
    assert!(!hardcalc::write(&a.doc).contains("footer"));
    ctrl(&mut a, 'z');
    assert_eq!(footer(&a)[0].1[1], "=sum");

    // Pasting several cells onto a footer is refused; a label must be unique.
    a.view.cursor = (5, 1);
    a.on_paste("1\t2");
    assert!(a.message.as_ref().is_some_and(|m| m.error));
    a.command("footer Total");
    assert!(a.message.as_ref().is_some_and(|m| m.error));
}

#[test]
fn footer_on_a_playground() {
    let mut a = App::new(PathBuf::from("x.hcmd"), hardcalc::parse("## P :: playground\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n").unwrap());
    assert!(a.fills());
    a.command("footer");
    assert!(!a.fills(), "a playground with a footer shows it below the + row");
    a.view.cursor.1 = 1;
    typ(&mut a, "=sum");
    key(&mut a, KeyCode::Enter);
    let screen = render(&mut a);
    assert!(screen.contains("Total") && screen.contains(" 6"), "{screen}");
}

#[test]
fn calculated_columns() {
    let mut a = app();
    let col = |a: &App, c: usize| a.wb.sheets[0].raw.iter().map(|r| r[c].clone()).collect::<Vec<_>>();
    let put = |a: &mut App, r: usize, c: usize, text: &str| {
        a.view.cursor = (r, c);
        typ(a, text);
        key(a, KeyCode::Enter);
    };

    // Share (D) holds the same formula in every row: a new one goes into all of them.
    put(&mut a, 1, 3, "=B2/sum(B)*2");
    assert_eq!(col(&a, 3), ["=B1/sum(B)*2", "=B2/sum(B)*2", "=B3/sum(B)*2"]);
    // A value is an exception for its cell only.
    put(&mut a, 1, 3, "0,5");
    assert_eq!(col(&a, 3), ["=B1/sum(B)*2", "0.5", "=B3/sum(B)*2"]);
    // A formula changes the whole column again, the value stays.
    put(&mut a, 0, 3, "=B1*2");
    assert_eq!(col(&a, 3), ["=B1*2", "0.5", "=B3*2"]);
    // One undo step.
    ctrl(&mut a, 'z');
    assert_eq!(col(&a, 3), ["=B1/sum(B)*2", "0.5", "=B3/sum(B)*2"]);
    ctrl(&mut a, 'y');

    // Gross (E) has different formulas on purpose: only the cell changes.
    put(&mut a, 0, 4, "=Amount*2");
    assert_eq!(col(&a, 4), ["=Amount*2", "=Amount * 1.19", "=Amount * 119%"]);

    // A new row on the + row gets the column's formula.
    put(&mut a, 3, 0, "Wasser");
    assert_eq!(col(&a, 3)[3], "=B4*2");
    assert_eq!(col(&a, 4)[3], "", "no calculated column");

    // Inserting a row: the rows below move, references follow, the new row calculates.
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Insert);
    assert_eq!(col(&a, 3), ["=B1*2", "=B2*2", "0.5", "=B4*2", "=B5*2"]);
    assert_eq!(col(&a, 0)[1], "");

    // A new column: references to the columns right of it follow.
    a.view.cursor = (0, 0);
    a.command("addcol Net");
    assert_eq!(col(&a, 4), ["=C1*2", "=C2*2", "0.5", "=C4*2", "=C5*2"]);
    // A formula in an empty column fills it.
    put(&mut a, 0, 1, "=Amount / 1.19");
    assert!(col(&a, 1).iter().all(|f| f == "=Amount / 1.19"), "{:?}", col(&a, 1));
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert_eq!(col(&a, 3), ["=B1*2", "=B2*2", "0.5", "=B4*2", "=B5*2"], "addcol is one undo step");
}

#[test]
fn new_layer_from_the_ui() {
    let src = "## T :: matrix\n\n| A | B |\n|---|---|\n| 1 | -2 |\n| 3 | 4 |\n\nText\n";
    let mut a = App::new(PathBuf::from("x.hcmd"), hardcalc::parse(src).unwrap());
    a.command("layer new");
    assert!(a.message.as_ref().is_some_and(|m| m.error));

    a.command("layer new validate");
    assert_eq!(a.solo.as_deref(), Some("validate"));
    assert!(a.layer_names.iter().any(|n| n == "validate"));
    assert!(hardcalc::write(&a.doc).contains("| 4 |\n\n```layer validate\n```\n\nText"), "{}", hardcalc::write(&a.doc));

    // The layer view edits its rules; back in the normal view they act.
    a.view.cursor = (0, 1);
    typ(&mut a, ">= 0");
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.solo, None);
    assert!(hardcalc::write(&a.doc).contains("```layer validate\nB1: >= 0\n```"), "{}", hardcalc::write(&a.doc));
    a.view.cursor = (0, 1);
    assert!(a.cursor_problem().is_some_and(|p| p.starts_with("validate")));

    // An existing layer is not added twice; undo removes a new one.
    a.command("layer new validate");
    assert_eq!(hardcalc::write(&a.doc).matches("```layer validate").count(), 1);
    key(&mut a, KeyCode::Esc);
    a.command("layer new notes");
    assert!(hardcalc::write(&a.doc).contains("```layer notes\n```"));
    ctrl(&mut a, 'z');
    assert!(!hardcalc::write(&a.doc).contains("layer notes"));
}

const LAYERED: &str = "\
## Account :: matrix [January]

| Item | Inflow | Balance      |
|--------|--------|--------------|
| A      | 100    | =C1[-1] + B1 |
| B      | 10     | =C2[-1] + B2 |

```slice February
| Inflow |
|--------|
| 50     |
| 20     |
```
";

#[test]
fn data_layers_in_the_ui() {
    let mut a = App::new(PathBuf::from("k.hcmd"), hardcalc::parse(LAYERED).unwrap());
    assert_eq!(a.corner(), "a January");
    // Esc leads out of a data layer back to the table.
    a.command("z b");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.shown_layer(), 0);
    let screen = render(&mut a);
    assert!(screen.contains("a January") && screen.contains("b February"), "{screen}");

    // Alt+↓ goes one layer down the stack; the formulas of the table work there.
    a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(a.shown_layer(), 1);
    a.view.cursor = (0, 2);
    assert_eq!(a.cursor_label(), "C1b");
    assert!(render(&mut a).contains("150"));
    assert_eq!(a.cursor_raw(), "=C1[-1] + B1", "inherited formula");

    // Typing writes the layer's own cell; Del gives it back to the table.
    a.view.cursor = (0, 1);
    typ(&mut a, "60");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.table().layers[0].rows[0][1], "60");
    assert_eq!(a.table().rows[0][1], "100");
    assert!(render(&mut a).contains("160"));
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Delete);
    assert_eq!(a.table().layers[0].rows[0][1], "");

    // The table origin is the layer head: ↑/↓ there.
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Up);
    assert_eq!(a.on_header(), Some(Header::Corner));
    key(&mut a, KeyCode::Up);
    assert_eq!(a.shown_layer(), 0);
    key(&mut a, KeyCode::Down);
    assert_eq!(a.shown_layer(), 1);
    key(&mut a, KeyCode::Esc);

    // New, renamed and deleted layers; :z by letter or name.
    a.command("slice March");
    assert_eq!(a.shown_layer(), 2);
    a.command("z a");
    assert_eq!(a.shown_layer(), 0);
    a.command("z March");
    a.command("z rename Maerz");
    assert!(hardcalc::write(&a.doc).contains("```slice Maerz"));
    a.command("z delete");
    assert_eq!(a.shown_layer(), 1);
    assert!(!hardcalc::write(&a.doc).contains("Maerz"));
    ctrl(&mut a, 'z');
    assert!(hardcalc::write(&a.doc).contains("Maerz"));
    a.command("slice b");
    assert!(a.message.as_ref().is_some_and(|m| m.error), "a letter is no name");

    // Sorting and inserting rows keep the layers in step.
    a.command("z February");
    a.view.cursor = (0, 1);
    a.command("sort desc");
    assert_eq!(a.table().rows[0][0], "A");
    a.command("sort");
    assert_eq!(a.table().rows[0][0], "B");
    assert_eq!(a.table().layers[0].rows[0][1], "20", "the layer row moved with its row");
    a.view.cursor = (0, 0);
    a.command("addrow");
    let zugang: Vec<&str> = a.table().layers[0].rows.iter().map(|r| r[1].as_str()).collect();
    assert_eq!(zugang, ["20", "", ""], "B keeps its 20, the new row is empty, A's 50 was cleared above");
    assert_eq!(a.table().rows[2][2], "=C3[-1] + B3", "references follow the moved row");
}

#[test]
fn calculated_columns_in_slices() {
    let src = "## Account :: matrix [January]\n\n| Item | Inflow | Double |\n|---|---|---|\n| A | 100 | =B1 * 2 |\n| B | 10 | =B2 * 2 |\n\n```slice February\n| Inflow |\n|---|\n| 50 |\n| 20 |\n```\n";
    let mut a = App::new(PathBuf::from("k.hcmd"), hardcalc::parse(src).unwrap());
    let layer = |a: &App, c: usize| a.table().layers[0].rows.iter().map(|r| r.get(c).cloned().unwrap_or_default()).collect::<Vec<_>>();
    a.command("z February");
    // Row names shine through a slice.
    assert!(render(&mut a).contains(" A "));
    // A formula in a calculated column of the slice: its whole column there; the table keeps its own.
    a.view.cursor = (1, 2);
    typ(&mut a, "=B2 * 3");
    key(&mut a, KeyCode::Enter);
    assert_eq!(layer(&a, 2), ["=B1 * 3", "=B2 * 3"]);
    assert_eq!(a.table().rows[0][2], "=B1 * 2");
    // A new row – typed in the slice or in the table – gets both formulas.
    a.view.cursor = (2, 1);
    typ(&mut a, "5");
    key(&mut a, KeyCode::Enter);
    assert_eq!(layer(&a, 2)[2], "=B3 * 3");
    assert_eq!(a.table().rows[2][2], "=B3 * 2");
    key(&mut a, KeyCode::Esc);
    a.view.cursor = (3, 0);
    typ(&mut a, "D");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.table().rows[3][2], "=B4 * 2");
    assert_eq!(layer(&a, 2)[3], "=B4 * 3");
    // A value stays an exception in the slice.
    a.command("z b");
    a.view.cursor = (0, 2);
    typ(&mut a, "7");
    key(&mut a, KeyCode::Enter);
    a.view.cursor = (1, 2);
    typ(&mut a, "=B2 * 4");
    key(&mut a, KeyCode::Enter);
    assert_eq!(layer(&a, 2), ["7", "=B2 * 4", "=B3 * 4", "=B4 * 4"]);
}

#[test]
fn moving_printing_and_laying_out_slices() {
    use hardcalc::print::{PageSetup, Placed, PrintItem};
    let src = format!(
        "{LAYERED}\n```scenario Plan\n| Inflow |\n|---|\n| 80 |\n```\n\nTarget {{=Account!B1c}}\n\n```layout L\n1-6 / 1-4  table Account   z: Plan\n7-12 / 1-4  table Account\n1-6 / 6-8  table Account   z: May\n```\n"
    );
    let mut a = App::new(PathBuf::from("k.hcmd"), hardcalc::parse(&src).unwrap());
    // Layout: one slice or scenario per element.
    let i = a.layout_blocks().iter().position(|(n, _)| n == "L").unwrap();
    let spec = a.raster_spec(i);
    let placed = a.raster_elements(&spec, &PageSetup::default());
    let text = |p: &hardcalc::print::PlacedElement| match &p.content {
        Placed::Lines(lines) => lines.iter().flat_map(|l| l.spans.iter().map(|s| s.text.clone())).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    };
    assert!(text(&placed[0]).contains("c Plan") && text(&placed[0]).contains("80"), "{}", text(&placed[0]));
    assert!(text(&placed[1]).contains("a January") && text(&placed[1]).contains("100"), "{}", text(&placed[1]));
    assert!(text(&placed[2]).contains("no slice or scenario 'May'"), "{}", text(&placed[2]));

    // Printing shows the slice or scenario on screen, named in the title.
    a.command("z February");
    let items = a.print_items(false);
    let PrintItem::Table(t) = &items[0] else { panic!() };
    assert_eq!(t.title, "Account · b February");
    assert_eq!(t.rows[0][2].text, "150");
    // The table remembers its level across tab changes.
    a.command("table New");
    a.switch_sheet(0);
    assert_eq!(a.shown_layer(), 1);

    // - / + on the table origin move it in the stack; letters in formulas follow.
    a.view.cursor = (0, 0);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Char('+'));
    assert_eq!(a.shown_layer(), 2);
    assert_eq!(a.table().layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Plan", "February"]);
    assert!(hardcalc::write(&a.doc).contains("Target {=Account!B1b}"), "{}", hardcalc::write(&a.doc));
    key(&mut a, KeyCode::Char('+'));
    assert_eq!(a.shown_layer(), 2, "already last");
    a.command("z a");
    key(&mut a, KeyCode::Char('-'));
    assert!(a.message.as_ref().is_some_and(|m| m.error), "a stays first");
    ctrl(&mut a, 'z');
    assert_eq!(a.table().layers[0].name, "February");
}

#[test]
fn calc_tab_edits_the_sheet() {
    let mut a = app();
    a.command("calc");
    assert_eq!(a.view.kind, hardcalc::tui::app::ViewKind::Calc);
    // The + row: typing adds lines to a new ```hardcalc block; ASCII input is tidied.
    typ(&mut a, "gamma_M = 1,5");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "f = 30 kN/m^2 / gamma_M");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "f <= 25 kN/m^2");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "m = Expenses!B1 * 2");
    key(&mut a, KeyCode::Enter);
    let body = hardcalc::sheet::source_lines(&a.doc);
    assert_eq!(body[0], "γ_M = 1.5");
    assert_eq!(body[2], "f ≤ 25 kN/m^2");
    let screen = render(&mut a);
    for s in ["∑ calc", "Calculation", "f = 30 kN/m²/γ_M = 30/1.5 = 20.0 kN/m²", "η = 0.80 ✓"] {
        assert!(screen.contains(s), "{s}\n{screen}");
    }
    // The table's Rent (1200 €) – € is not a physical unit.
    assert!(screen.contains("unknown unit €"), "{screen}");
    // F8 deletes the line at the cursor.
    a.view.cursor = (3, 0);
    key(&mut a, KeyCode::F(8));
    assert_eq!(hardcalc::sheet::source_lines(&a.doc).len(), 3);
    // The calculation column is computed.
    a.view.cursor = (0, 1);
    typ(&mut a, "x");
    assert!(matches!(a.mode, Mode::Normal));
}

#[test]
fn calc_preview_typesets_formulas() {
    let src = include_str!("../examples/beam.hcmd");
    let mut a = App::new(PathBuf::from("beam.hcmd"), hardcalc::parse(src).unwrap());
    a.command("calc");
    a.command("preview");
    assert_eq!(a.other.as_ref().map(|v| v.kind), Some(hardcalc::tui::app::ViewKind::CalcPreview));
    let lines: Vec<String> = a.with_window(false, |a| a.calc_preview()).into_iter().map(|(l, _)| l).collect();
    let text = lines.join("\n");
    // f_yd = f_yk / γ_M as a fraction over three lines.
    let i = lines.iter().position(|l| l.contains("f   = ─")).unwrap_or_else(|| panic!("{text}"));
    assert!(lines[i].contains("─────"), "{text}");
    assert!(lines[i + 1].contains("γ"), "{text}");
    assert!(text.contains("η = 0.45 ✓"), "{text}");
    // One line per formula on request.
    a.command("formulas text");
    let lines: Vec<String> = a.with_window(false, |a| a.calc_preview()).into_iter().map(|(l, _)| l).collect();
    assert!(lines.contains(&"f_yd = f_yk/γ_M = 235/1.1 = 214 N/mm²".to_string()), "{lines:?}");
    render(&mut a);
}

#[test]
fn context_menu_by_key_sequence_mouse_and_shift_f10() {
    use hardcalc::tui::keymap::Cap;
    let mut a = app();
    render(&mut a);
    let rows = a.table().rows.len();
    // / R I: insert a row.
    typ(&mut a, "/");
    assert!(a.context.is_some());
    let screen = render(&mut a);
    assert!(screen.contains("Edit") && screen.contains("Column"), "{screen}");
    typ(&mut a, "ri");
    assert!(a.context.is_none());
    assert_eq!(a.table().rows.len(), rows + 1);
    // Entries show their keys (only those that work here).
    typ(&mut a, "/e");
    let screen = render(&mut a);
    assert!(screen.contains("Ctrl+C · F5"), "{screen}");
    key(&mut a, KeyCode::Esc);
    assert!(a.context.is_none());
    // Right-click on a cell: the cursor goes there, the menu opens there.
    render(&mut a);
    let (rect, _) = *a.hits.iter().find(|(_, h)| *h == Hit::Cell(2, 1)).unwrap();
    a.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE });
    assert_eq!(a.view.cursor, (2, 1));
    assert_eq!(a.context.map(|m| m.pos), Some((rect.x, rect.y + 1)));
    assert_eq!(a.caps.mouse, Cap::Confirmed);
    key(&mut a, KeyCode::Esc);
    // Shift+F10 arrives: it works here – the menu opens, F10 alone still quits.
    a.on_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT));
    assert!(a.context.is_some());
    assert_eq!(a.caps.shift_f10, Cap::Confirmed);
    key(&mut a, KeyCode::Esc);
    // '/ at the start types a slash.
    a.view.cursor = (0, 0);
    typ(&mut a, "'/2");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.table().rows[0][0], "/2");
}

#[test]
fn key_test_and_keys_help_page() {
    use hardcalc::tui::keymap::Cap;
    let mut a = app();
    a.command("keytest");
    assert_eq!(a.key_test, Some(0));
    // Menu key: pressed, nothing arrived.
    typ(&mut a, "n");
    assert_eq!(a.caps.menu_key, Cap::Impossible);
    // Shift+F10 arrives.
    a.on_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT));
    assert_eq!(a.caps.shift_f10, Cap::Confirmed);
    assert!(!a.quit && a.context.is_none());
    // Mouse: skipped.
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.key_test, None);
    assert_eq!(a.caps.mouse, Cap::Unknown);
    // The Keys page shows Shift+F10, not the Menu key, not right-click.
    fkey(&mut a, 1);
    key(&mut a, KeyCode::Char('7'));
    let screen = render_big(&mut a);
    assert!(screen.contains("7/7 · Keys"), "{screen}");
    let line = screen.lines().find(|l| l.contains("Context menu")).unwrap_or_else(|| panic!("{screen}"));
    assert!(line.contains("/ · Shift+F10") && !line.contains("Menu key") && !line.contains("right-click"), "{line}");
}

#[test]
fn status_line_follows_the_focused_window() {
    let mut a = app();
    ctrl(&mut a, 'o');
    let status = |a: &mut App| render(a).lines().nth(18).unwrap().to_string();
    assert!(status(&mut a).contains("double-click to edit"), "{}", status(&mut a));
    // The inspector on the right is focused: its keys, not the table's.
    key(&mut a, KeyCode::Tab);
    assert!(!a.left_focused);
    assert!(status(&mut a).contains("inspector: ↑↓ choose"), "{}", status(&mut a));
    key(&mut a, KeyCode::Tab);
    assert!(status(&mut a).contains("double-click to edit"));
}

#[test]
fn split_separator_drags_between_40_and_60_percent() {
    let mut a = app();
    ctrl(&mut a, 'o');
    render(&mut a);
    let sep = a.split_sep;
    assert_eq!(sep.x, 50);
    let mouse = |a: &mut App, kind, column| a.on_mouse(MouseEvent { kind, column, row: sep.y + 2, modifiers: KeyModifiers::NONE });
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), sep.x);
    assert!(a.split_drag);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 45);
    assert_eq!(a.split_percent, 45);
    // Beyond the limits it stops at 40 / 60.
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 5);
    assert_eq!(a.split_percent, 40);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 95);
    assert_eq!(a.split_percent, 60);
    mouse(&mut a, MouseEventKind::Up(MouseButton::Left), 95);
    assert!(!a.split_drag);
    render(&mut a);
    assert_eq!(a.split_sep.x, 60);
    // The drag did not move the focus.
    assert!(a.left_focused);
    // A plain drag elsewhere does not resize.
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 45);
    assert_eq!(a.split_percent, 60);
}

fn shaped(src: &str) -> App {
    App::new(PathBuf::from("x.hcmd"), hardcalc::parse(src).unwrap())
}

#[test]
fn shape_turns_a_playground_into_a_matrix_without_the_empty_edges() {
    let src = "## P :: playground\n\n| A | B | C | D |\n|---|---|---|---|\n|  |  |  |  |\n|  | 1 | =B2*2 |  |\n|  | 3 | 4 |  |\n|  |  |  |  |\n\n```layer comments\nC3: vier\n```\n\n## Q :: matrix\n\n| X |\n|---|\n| =P!C2 + sum(P!B2:B3) |\n";
    let mut a = shaped(src);
    a.view.cursor = (2, 2);
    command(&mut a, "shape matrix");
    assert!(!a.message.as_ref().unwrap().error, "{:?}", a.message);
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("## P :: matrix\n\n| A | B |\n"), "{text}");
    assert!(text.contains("| 1 | =A1*2 |\n| 3 | 4 |\n\n"), "{text}");
    assert!(text.contains("B2: vier"), "{text}");
    assert!(text.contains("=P!B1 + sum(P!A1:A2)"), "{text}");
    assert_eq!(a.view.cursor, (1, 1));
    assert_eq!(a.wb.value((1, 0, 0)).display(None, false), "6");
    assert!(!a.fills());

    // And back: only the shape changes.
    command(&mut a, "shape playground");
    assert!(hardcalc::write(&a.doc).contains("## P :: playground\n"));
    assert!(a.fills());
}

#[test]
fn shape_needs_one_connected_area() {
    // Diagonal neighbours belong together, a gap separates.
    let mut a = shaped("## P :: playground\n\n| A | B | C | D |\n|---|---|---|---|\n| 1 |  |  |  |\n|  | 2 |  | 9 |\n");
    command(&mut a, "shape matrix");
    let m = a.message.clone().unwrap();
    assert!(m.error && m.text.contains("2 separate areas (A1:B2, D2)"), "{}", m.text);
    assert_eq!(a.table().shape, hardcalc::model::Shape::Playground);
    assert!(!a.dirty);

    // A comment on a cell counts as content.
    let mut a = shaped("## P :: playground\n\n| A | B | C |\n|---|---|---|\n| 1 |  |  |\n\n```layer comments\nC3: Note\n```\n");
    command(&mut a, "shape matrix");
    assert!(a.message.as_ref().unwrap().text.contains("A1, C3"), "{:?}", a.message);

    // Named columns join the area through the header row.
    let mut a = shaped("## P :: playground\n\n| Name | Note | Price |\n|---|---|---|\n| x |  | 1 |\n");
    command(&mut a, "shape matrix");
    assert!(!a.message.as_ref().unwrap().error, "{:?}", a.message);
    assert!(hardcalc::write(&a.doc).contains("## P :: matrix\n\n| Name | Note | Price |\n"));
}

#[test]
fn shape_keeps_references_into_the_empty_edge() {
    let mut a = shaped("## P :: playground\n\n| A | B |\n|---|---|\n|  |  |\n|  | =A1+1 |\n");
    command(&mut a, "shape matrix");
    assert!(a.message.as_ref().unwrap().error && a.message.as_ref().unwrap().text.contains("refers to the empty"), "{:?}", a.message);
    assert_eq!(a.table().shape, hardcalc::model::Shape::Playground);
}

fn format_block(a: &App) -> String {
    let text = hardcalc::write(&a.doc);
    match text.split("```layer format\n").nth(1) {
        Some(rest) => rest.split("```").next().unwrap_or("").to_string(),
        None => String::new(),
    }
}

const PLAIN: &str = "## T :: matrix\n\n| Name | Value |\n|------|------|\n| a    | 1    |\n| b    | 2    |\n| c    | 3    |\n";

#[test]
fn ctrl_b_toggles_bold_of_the_cell() {
    let mut a = App::new(PathBuf::from("f.hcmd"), hardcalc::parse(PLAIN).unwrap());
    render(&mut a);
    a.view.cursor = (1, 1);
    ctrl(&mut a, 'b');
    assert_eq!(format_block(&a), "B2: bold\n");
    assert!(a.cell_view(1, 1).style.bold);
    // Again: off, and the empty block goes.
    ctrl(&mut a, 'b');
    assert!(!hardcalc::write(&a.doc).contains("layer format"), "{}", hardcalc::write(&a.doc));
    // One step each: undo brings bold back.
    ctrl(&mut a, 'z');
    assert_eq!(format_block(&a), "B2: bold\n");
}

#[test]
fn columns_rows_and_exceptions() {
    let mut a = App::new(PathBuf::from("f.hcmd"), hardcalc::parse(PLAIN).unwrap());
    render(&mut a);
    // The whole column B.
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Up);
    command(&mut a, "bold");
    assert_eq!(format_block(&a), "B: bold\n");
    // One cell of it back to normal.
    key(&mut a, KeyCode::Down);
    a.view.cursor = (1, 1);
    command(&mut a, "bold");
    assert_eq!(format_block(&a), "B: bold\nB2: nobold\n");
    assert!(!a.cell_view(1, 1).style.bold);
    assert!(a.cell_view(2, 1).style.bold);
    // Bold for the whole column again: the exception inside goes, as in Excel.
    a.view.cursor = (0, 1);
    key(&mut a, KeyCode::Up);
    command(&mut a, "bold");
    assert_eq!(format_block(&a), "", "column off, exception removed");
    command(&mut a, "bold");
    assert_eq!(format_block(&a), "B: bold\n");
    // A row header: a row rule.
    key(&mut a, KeyCode::Down);
    a.view.cursor = (2, 0);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.on_header(), Some(Header::Row));
    command(&mut a, "italic");
    assert_eq!(format_block(&a), "B: bold\n3: italic\n");
}

#[test]
fn colors_and_plain() {
    let mut a = App::new(PathBuf::from("f.hcmd"), hardcalc::parse(PLAIN).unwrap());
    render(&mut a);
    a.view.cursor = (0, 0);
    a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    command(&mut a, "fg red");
    command(&mut a, "bg #ffeeaa");
    command(&mut a, "underline");
    assert_eq!(format_block(&a), "A1:B2: fg red bg #ffeeaa underline\n");
    command(&mut a, "fg chartreuse");
    assert!(a.message.as_ref().is_some_and(|m| m.error), "unknown color");
    command(&mut a, "fg none");
    assert_eq!(format_block(&a), "A1:B2: bg #ffeeaa underline\n");
    command(&mut a, "plain");
    assert_eq!(format_block(&a), "");
}

#[test]
fn format_through_the_context_menu() {
    let mut a = App::new(PathBuf::from("f.hcmd"), hardcalc::parse(PLAIN).unwrap());
    render(&mut a);
    a.view.cursor = (2, 0);
    typ(&mut a, "/");
    typ(&mut a, "fi");
    assert_eq!(format_block(&a), "A3: italic\n");
    typ(&mut a, "/fb");
    assert_eq!(format_block(&a), "A3: italic bold\n");
    // The text color asks for the color on the command line.
    typ(&mut a, "/fc");
    assert!(matches!(&a.mode, Mode::Command { buf } if buf == "fg "), "{:?}", a.mode);
}


#[test]
fn decimal_comma_only_on_screen() {
    // With a German locale numbers are shown and edited with the comma; the
    // file keeps the point.
    hardcalc::locale::set_decimal_comma_for_this_thread(true);
    let mut a = app();
    assert!(render(&mut a).contains("1200,00"));
    a.view.cursor = (1, 1);
    key(&mut a, KeyCode::Enter);
    assert!(matches!(&a.mode, Mode::Edit { buf, .. } if buf == "85,50"), "{:?}", a.mode);
    // Leaving the edit unchanged changes nothing.
    key(&mut a, KeyCode::Esc);
    assert!(!a.dirty);
    typ(&mut a, "99,5");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.wb.sheets[0].raw[1][1], "99.5");
    assert!(hardcalc::write(&a.doc).contains("| Power | 99.5 |"), "{}", hardcalc::write(&a.doc));
    assert!(render(&mut a).contains("99,50"));

    // Files written with the comma are read and written with the point.
    let doc = hardcalc::parse("## T :: matrix\n\n| a | b:decimal(2) |\n|---|---|\n| x | 1,5 |\n\n```hardcalc\nq = 1,35 kN\n```\n").unwrap();
    let out = hardcalc::write(&doc);
    assert!(out.contains("| x | 1.5 |") && out.contains("q = 1.35 kN"), "{out}");
}


fn cmd(a: &mut App, text: &str) {
    typ(a, text);
    key(a, KeyCode::Enter);
}

#[test]
fn bg_can_be_removed_again() {
    let mut a = app();
    cmd(&mut a, ":bg blue");
    assert!(hardcalc::write(&a.doc).contains("bg blue"), "{}", hardcalc::write(&a.doc));
    cmd(&mut a, ":bg");
    assert!(!hardcalc::write(&a.doc).contains("bg blue"), "{}", hardcalc::write(&a.doc));
}

#[test]
fn spaces_in_a_formula_stay_in_the_editor() {
    let mut a = app();
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "= 1 + 2 * (3 + 4) ");
    assert!(matches!(a.mode, Mode::Edit { .. }), "{:?}", a.mode);
    key(&mut a, KeyCode::Enter);
    let _ = render(&mut a);
}

#[test]
fn arrows_move_the_caret_in_a_formula_with_spaces() {
    let mut a = app();
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Home);
    for _ in 0..20 {
        key(&mut a, KeyCode::Backspace);
        key(&mut a, KeyCode::Delete);
    }
    typ(&mut a, "=A1 + B1");
    for _ in 0..3 {
        key(&mut a, KeyCode::Left);
    }
    let Mode::Edit { buf, cursor } = &a.mode else { panic!("left the editor") };
    assert_eq!((buf.as_str(), *cursor), ("=A1 + B1", 5), "caret must stay in the text");
    assert!(a.point_range().is_none());
}

fn bg_app(rule: &str) -> App {
    let src = format!("## T :: matrix\n\n| A | B |\n|---|---|\n| 1 | x |\n| 2 | y |\n\n```layer format\n{rule}\n```\n");
    App::new(PathBuf::from("t.hcmd"), hardcalc::parse(&src).unwrap())
}

#[test]
fn bg_removes_colors_from_other_rules() {
    for rule in ["A1:A2: bg green", "A: bg green", "A1: bg green", "row[B=x]: bg green", "1: bg green"] {
        let mut a = bg_app(rule);
        cmd(&mut a, ":bg");
        let out = hardcalc::write(&a.doc);
        let (r, c) = a.view.cursor;
        let (mut wb, _) = hardcalc::Workbook::from_document(&a.doc);
        let att = hardcalc::layers::attach(&a.doc, &wb);
        let style = hardcalc::layers::Formats::new(&att, "format").style(&mut wb, (0, r, c));
        assert!(style.bg.is_none(), "{rule}: bg survived\n{out}");
    }
}

#[test]
fn bg_on_a_selection_can_be_removed_after_reload() {
    let mut a = app();
    a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    cmd(&mut a, ":bg blue");
    let text = hardcalc::write(&a.doc);
    let mut b = App::new(PathBuf::from("budget.hcmd"), hardcalc::parse(&text).unwrap());
    a.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    b.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    cmd(&mut b, ":bg");
    let out = hardcalc::write(&b.doc);
    assert!(!out.contains("bg blue"), "{text}\n---\n{out}");
}

fn wrap_app(rule: &str) -> App {
    let src = format!(
        "## T :: matrix\n\n| N | Note |\n|---|------|\n| 1 | the quick brown fox jumps over the lazy dog |\n| 2 | short |\n| 3 | last |\n\n```layer format\n{rule}\n```\n"
    );
    App::new(PathBuf::from("t.hcmd"), hardcalc::parse(&src).unwrap())
}

#[test]
fn wrapped_cells_span_several_lines() {
    let mut a = wrap_app("Note: wrap 16");
    let screen = render(&mut a);
    assert!(screen.contains("the quick brown"), "{screen}");
    assert!(screen.contains("fox jumps over"), "{screen}");
    assert!(screen.contains("the lazy dog"), "{screen}");
    // Rows 2 and 3 follow below the three lines of row 1.
    let lines: Vec<&str> = screen.lines().collect();
    let at = |t: &str| lines.iter().position(|l| l.contains(t)).unwrap();
    assert_eq!(at("short") - at("the quick brown"), 3, "{screen}");
    assert_eq!(at("last") - at("short"), 1, "{screen}");
}

#[test]
fn without_wrap_long_text_is_cut() {
    let mut a = wrap_app("Note: bold");
    let screen = render(&mut a);
    assert!(!screen.contains("the lazy dog"), "{screen}");
}

#[test]
fn cursor_row_stays_visible_with_wrapped_rows() {
    let mut a = wrap_app("Note: wrap 16");
    for _ in 0..2 {
        key(&mut a, KeyCode::Down);
    }
    let screen = render(&mut a);
    assert!(screen.contains("last"), "{screen}");
}

#[test]
fn scrolling_shows_the_whole_wrapped_cursor_row() {
    let mut src = String::from("## T :: matrix\n\n| N | Note |\n|---|------|\n");
    for i in 1..=30 {
        src.push_str(&format!("| {i} | alpha beta gamma delta epsilon{i} zeta eta theta |\n"));
    }
    src.push_str("\n```layer format\nNote: wrap 12\n```\n");
    let mut a = App::new(PathBuf::from("t.hcmd"), hardcalc::parse(&src).unwrap());
    for _ in 0..29 {
        key(&mut a, KeyCode::Down);
    }
    let screen = render(&mut a);
    assert!(screen.contains("epsilon30"), "{screen}");
    assert!(screen.lines().filter(|l| l.contains("theta")).count() >= 1, "{screen}");
    let last = screen.lines().filter(|l| l.contains("30")).count();
    assert!(last >= 1, "{screen}");
}

#[test]
fn long_input_wraps_in_the_formula_bar() {
    let mut a = app();
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::End);
    for _ in 0..30 {
        key(&mut a, KeyCode::Backspace);
    }
    let long = format!("={}", vec!["1"; 90].join("+"));
    typ(&mut a, &long);
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    term.draw(|f| ui::draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer();
    let rows: Vec<String> = (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect();
    // The text continues on the second and third line instead of running off.
    assert!(rows[1].contains("1+1+1"), "{rows:#?}");
    assert!(rows[2].contains("1+1+1") && !rows[2].contains("Item"), "{rows:#?}");
    let end = rows.iter().take(8).map(|r| r.matches('1').count()).sum::<usize>();
    assert!(end >= 90, "{end}\n{rows:#?}");
}

#[test]
fn wrap_command_writes_and_removes_the_rule() {
    let mut a = app();
    cmd(&mut a, ":wrap 12");
    assert!(hardcalc::write(&a.doc).contains("wrap 12"), "{}", hardcalc::write(&a.doc));
    cmd(&mut a, ":wrap");
    let out = hardcalc::write(&a.doc);
    assert!(out.contains("wrap") && !out.contains("wrap 12"), "{out}");
    cmd(&mut a, ":wrap on");
    let out = hardcalc::write(&a.doc);
    assert!(out.contains("wrap") && !out.contains("wrap 12"), "{out}");
    cmd(&mut a, ":wrap off");
    assert!(!hardcalc::write(&a.doc).contains("wrap"), "{}", hardcalc::write(&a.doc));
    cmd(&mut a, ":wrap x");
}

#[test]
fn raster_layout_tables_wrap_too() {
    let src = "## T :: matrix\n\n| N | Note |\n|---|------|\n| 1 | the quick brown fox jumps over the lazy dog |\n| 2 | short |\n\n```layer format\nNote: wrap 12\n```\n\n```layout L\npage: A4 portrait\nfont: 9\ngrid: 12\n1-12 / 1-12 table T\n```\n";
    let mut a = App::new(PathBuf::from("t.hcmd"), hardcalc::parse(src).unwrap());
    let layout = a.layout(&hardcalc::print::PageSetup { scope: hardcalc::print::Scope::Layout(0), ..Default::default() });
    let text: Vec<String> = layout.pages[0].iter().map(|l| l.plain()).collect();
    let at = |t: &str| text.iter().position(|l| l.contains(t)).unwrap_or_else(|| panic!("{t}: {text:#?}"));
    assert_eq!(at("short") - at("the quick"), 4, "{text:#?}");
    assert!(text.iter().any(|l| l.contains("lazy dog")));
}

#[test]
fn formula_bar_stays_one_line_while_navigating() {
    let mut a = app();
    key(&mut a, KeyCode::Enter);
    typ(&mut a, &format!("={}", vec!["1"; 90].join("+")));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Up);
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    term.draw(|f| ui::draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer();
    let row = |y: u16| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>();
    assert!(!row(2).contains("1+1+1"), "{}", row(2));
    assert!(row(1).trim_end().contains('…'), "{}", row(1));
}

#[test]
fn compact_is_the_default_and_aligned_can_be_chosen() {
    let mut a = app();
    let out = hardcalc::write(&a.doc);
    assert!(out.contains("| Rent | 1200.00 | paid |"), "{out}");
    assert!(!out.contains("aligned"), "{out}");
    cmd(&mut a, ":compact off");
    let out = hardcalc::write(&a.doc);
    assert!(out.contains(":: matrix[*, 5] aligned"), "{out}");
    assert!(out.contains("| Rent     |"), "{out}");
    cmd(&mut a, ":compact");
    assert!(!hardcalc::write(&a.doc).contains("aligned"));
    cmd(&mut a, ":compact maybe");
}
