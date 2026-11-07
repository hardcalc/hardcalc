use hardcalc::config::{ConfigFile, Path};
use hardcalc::tui::config::app::{ChooseFor, ConfApp, Confirm, Focus, Mode, Then};
use hardcalc::tui::config::ui;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::path::PathBuf;

const EXAMPLE: &str = include_str!("../examples/backup.toml");

fn app() -> ConfApp {
    ConfApp::new(PathBuf::from("backup.toml"), ConfigFile::parse(EXAMPLE).unwrap())
}

fn render(app: &mut ConfApp) -> String {
    let mut term = Terminal::new(TestBackend::new(110, 32)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(app: &mut ConfApp, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut ConfApp, c: char) {
    app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn typ(app: &mut ConfApp, text: &str) {
    for c in text.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// Opens the node at `path` and puts the cursor on the form row `label`.
fn goto(app: &mut ConfApp, path: &str, label: &str) {
    app.select(if path.is_empty() { Path::root() } else { Path::parse(path).unwrap() });
    app.focus = Focus::Form;
    app.row = app.rows().iter().position(|r| r.label == label).unwrap_or_else(|| panic!("no row {label}"));
}

fn value(app: &ConfApp, path: &str) -> String {
    app.value(&Path::parse(path).unwrap()).map(|v| v.to_string().trim().to_string()).unwrap_or_default()
}

/// A file in the temp folder, removed at the end of the test.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, content: &str) -> TempFile {
        let p = std::env::temp_dir().join(format!("hardconf-{}-{name}", std::process::id()));
        std::fs::write(&p, content).unwrap();
        TempFile(p)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn tree_and_form_show_the_file() {
    let mut a = app();
    let screen = render(&mut a);
    assert!(screen.contains("▾ targets  Storage target"), "{screen}");
    assert!(screen.contains("revision  r-5f2c91d0  fixed"), "{screen}");
    assert!(screen.contains("Edit the values, save and close hardconf."), "{screen}");
    // Down to nas1 in the tree: its form and the comment above/below it in the file.
    for _ in 0..3 {
        key(&mut a, KeyCode::Down);
    }
    let screen = render(&mut a);
    assert!(screen.contains("Storage target: nas1"), "{screen}");
    assert!(screen.contains("tls_name         —  not set"), "{screen}");
    assert!(screen.contains("Used by: photos"), "{screen}");
}

#[test]
fn typing_replaces_a_value_and_errors_show() {
    let mut a = app();
    goto(&mut a, "targets.nas1", "ssh_port");
    typ(&mut a, "1445");
    assert!(matches!(&a.mode, Mode::Edit { buf, .. } if buf == "1445"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(value(&a, "targets.nas1.ssh_port"), "1445");
    assert!(a.modified());

    typ(&mut a, "0");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.counts().0, 1);
    assert_eq!(a.message.as_ref().map(|m| (m.text.as_str(), m.error)), Some(("0 outside 1..65535", true)));
    assert!(render(&mut a).contains("● 1 error"));

    // Not an integer: refused, still editing.
    typ(&mut a, "abc");
    key(&mut a, KeyCode::Enter);
    assert!(matches!(a.mode, Mode::Edit { .. }));
    key(&mut a, KeyCode::Esc);
    assert_eq!(value(&a, "targets.nas1.ssh_port"), "0");

    // Del: back to the default, the key disappears from the file.
    key(&mut a, KeyCode::Delete);
    assert_eq!(value(&a, "targets.nas1.ssh_port"), "");
    assert!(render(&mut a).contains("ssh_port         22  default"));
    ctrl(&mut a, 'z');
    assert_eq!(value(&a, "targets.nas1.ssh_port"), "0");
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert!(!a.modified(), "undo back to the original");
}

#[test]
fn enum_choice_and_conditions() {
    let mut a = app();
    goto(&mut a, "targets.nas1", "tls");
    key(&mut a, KeyCode::Enter);
    let Mode::Choose { options, cursor, .. } = &a.mode else { panic!("choice list") };
    assert_eq!(options.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["verify", "ca", "pin"]);
    assert_eq!(*cursor, 2, "starts on the current value");
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Enter);
    assert_eq!(value(&a, "targets.nas1.tls"), "\"ca\"");
    // tls_fingerprint is only used with pin: its value now has no effect.
    assert!(render(&mut a).contains("tls_fingerprint  \"\"  has no effect, only used when tls = pin"));
}

#[test]
fn bool_toggles_and_secrets_are_hidden() {
    let mut a = app();
    goto(&mut a, "jobs.photos", "skip_hidden");
    key(&mut a, KeyCode::Enter);
    assert_eq!(value(&a, "jobs.photos.skip_hidden"), "true");
    key(&mut a, KeyCode::Char(' '));
    assert_eq!(value(&a, "jobs.photos.skip_hidden"), "false");

    let screen = render(&mut a);
    assert!(!screen.contains("tok_3c9e71a0b5d2f846"), "token hidden");
    a.fkey(3);
    assert!(render(&mut a).contains("tok_3c9e71a0b5d2f846"), "F3 shows it");
    // A special value is shown with its meaning, never hidden.
    goto(&mut a, "accounts.archive", "password");
    assert!(render(&mut a).contains("********  keep the stored password"));
}

#[test]
fn list_elements_are_added_and_removed() {
    let mut a = app();
    goto(&mut a, "jobs.photos.include", "both");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "*.jpg");
    key(&mut a, KeyCode::Enter);
    assert_eq!(value(&a, "jobs.photos.include.both"), "[\"*.jpg\"]");
    // The text form keeps the dotted keys of the file.
    assert!(a.cfg.to_text().contains("include.both    = [\"*.jpg\"]\n"));
    a.row -= 1;
    key(&mut a, KeyCode::Delete);
    assert_eq!(value(&a, "jobs.photos.include.both"), "[]");
}

#[test]
fn missing_struct_is_created_on_first_edit() {
    let mut a = app();
    goto(&mut a, "jobs.documents.include", "upload");
    assert!(render(&mut a).contains("(not in the file: defaults)"));
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "*.pdf");
    key(&mut a, KeyCode::Enter);
    assert_eq!(value(&a, "jobs.documents.include.upload"), "[\"*.pdf\"]");
}

#[test]
fn save_removes_diagnostics_and_quit_never_writes() {
    let text = format!("#%error jobs.photos.notify \"not a valid email address\"\n{EXAMPLE}");
    let f = TempFile::new("save.toml", &text);
    let mut a = ConfApp::open(&f.0).unwrap();
    assert!(a.findings(&Path::parse("jobs.photos.notify").unwrap()).iter().any(|d| d.message == "not a valid email address"));
    assert!(!a.modified(), "removing diagnostics alone is no change");

    goto(&mut a, "jobs.photos", "max_per_day");
    typ(&mut a, "50");
    key(&mut a, KeyCode::Enter);
    a.fkey(10);
    assert_eq!(a.mode, Mode::Confirm(Confirm::Unsaved(Then::Quit)));
    key(&mut a, KeyCode::Esc);
    assert!(!a.quit);
    assert_eq!(std::fs::read_to_string(&f.0).unwrap(), text, "nothing written yet");

    ctrl(&mut a, 's');
    let saved = std::fs::read_to_string(&f.0).unwrap();
    assert!(!saved.contains("#%error"));
    assert_eq!(saved, EXAMPLE.replace("max_per_day  = 100", "max_per_day  = 50"));
    assert!(a.findings(&Path::parse("jobs.photos.notify").unwrap()).is_empty());
    a.fkey(10);
    assert!(a.quit, "saved: quits without asking");
}

#[test]
fn quit_without_changes_leaves_the_file_alone() {
    let text = format!("{EXAMPLE}#%warning \"stays\"\n");
    let f = TempFile::new("quit.toml", &text);
    let mut a = ConfApp::open(&f.0).unwrap();
    ctrl(&mut a, 'q');
    assert!(a.quit);
    assert_eq!(std::fs::read_to_string(&f.0).unwrap(), text);
}

#[test]
fn save_with_errors_asks() {
    let f = TempFile::new("errors.toml", EXAMPLE);
    let mut a = ConfApp::open(&f.0).unwrap();
    goto(&mut a, "targets.nas1", "host");
    typ(&mut a, "kein host");
    key(&mut a, KeyCode::Enter);
    a.fkey(2);
    assert_eq!(a.mode, Mode::Confirm(Confirm::SaveWithErrors));
    key(&mut a, KeyCode::Char('n'));
    assert_eq!(std::fs::read_to_string(&f.0).unwrap(), EXAMPLE);
    a.fkey(2);
    key(&mut a, KeyCode::Char('y'));
    assert!(std::fs::read_to_string(&f.0).unwrap().contains("host      = \"kein host\""));
}

#[test]
fn file_without_schema_is_a_generic_tree() {
    let mut a = ConfApp::new(PathBuf::from("plain.toml"), ConfigFile::parse("name = \"x\"\nport = 80\n\n[db]\nurl = \"pg://\"\n").unwrap());
    let screen = render(&mut a);
    assert!(screen.lines().next().unwrap().ends_with(" plain TOML"), "{screen}");
    assert!(screen.contains("db"));
    goto(&mut a, "", "port");
    typ(&mut a, "8080");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.cfg.to_text(), "name = \"x\"\nport = 8080\n\n[db]\nurl = \"pg://\"\n");
    goto(&mut a, "", "name");
    typ(&mut a, "42");
    key(&mut a, KeyCode::Enter);
    assert_eq!(value(&a, "name"), "\"42\"", "text stays text");
}

#[test]
fn renders_at_any_size_in_every_mode() {
    let setups: [fn(&mut ConfApp); 13] = [
        |_| {},
        |a| {
            goto(a, "targets.nas1", "host");
            key(a, KeyCode::Enter);
        },
        |a| {
            goto(a, "targets.nas1", "tls");
            key(a, KeyCode::Enter);
        },
        |a| {
            goto(a, "targets.nas1", "tls_ca");
            typ(a, "x");
            key(a, KeyCode::Enter);
            typ(a, "second line");
        },
        |a| a.fkey(1),
        |a| {
            a.select(Path::parse("jobs").unwrap());
            a.fkey(7);
            typ(a, "x");
        },
        |a| {
            a.select(Path::parse("targets.nas1").unwrap());
            a.fkey(8);
        },
        |a| a.fkey(5),
        |a| {
            a.select(Path::parse("targets.nas1").unwrap());
            a.fkey(6);
        },
        |a| goto(a, "jobs.photos", "notes"),
        |a| {
            a.select(Path::root());
            a.fkey(7);
            typ(a, "new");
            key(a, KeyCode::Enter);
        },
        |a| {
            goto(a, "jobs.photos", "max_per_day");
            a.fkey(9);
        },
        |a| {
            goto(a, "jobs.photos", "max_per_day");
            typ(a, "5");
            key(a, KeyCode::Enter);
            a.fkey(10);
        },
    ];
    for setup in setups {
        for (w, h) in [(1, 1), (10, 3), (20, 6), (40, 10), (80, 24), (200, 60)] {
            let mut a = app();
            setup(&mut a);
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| ui::draw(f, &mut a)).unwrap();
        }
    }
    // The multi-line editor really has two lines.
    let mut a = app();
    setups[3](&mut a);
    assert!(matches!(&a.mode, Mode::Edit { buf, multiline: true, .. } if buf == "x\nsecond line"));
    key(&mut a, KeyCode::Tab);
    assert!(a.cfg.to_text().contains("tls_ca = \"\"\"\nx\nsecond line\"\"\""), "{}", a.cfg.to_text());
}

fn fkey_bar(a: &mut ConfApp) -> String {
    render(a).lines().last().unwrap().to_string()
}

#[test]
fn new_entries_ask_for_a_checked_name() {
    let mut a = app();
    a.select(Path::parse("jobs").unwrap());
    assert!(fkey_bar(&mut a).contains("7New") && !fkey_bar(&mut a).contains("8Delete"));
    a.fkey(7);
    assert!(matches!(&a.mode, Mode::NewKey { map, .. } if map.to_string() == "jobs"));
    typ(&mut a, "New");
    assert!(render(&mut a).contains("new entry in jobs – Name: New"));
    key(&mut a, KeyCode::Enter);
    assert!(matches!(a.mode, Mode::NewKey { .. }), "bad name: still asking");
    assert!(a.message.as_ref().unwrap().text.contains("does not match"));
    ctrl(&mut a, 'u');
    typ(&mut a, "photos");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().unwrap().text.contains("exists already"));
    ctrl(&mut a, 'u');
    typ(&mut a, "new");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(a.selected.to_string(), "jobs.new");
    assert_eq!(a.focus, Focus::Form);
    assert!(a.cfg.to_text().contains("\n[jobs.new]\n"));
    assert!(render(&mut a).contains("Job: new"));
    // From an entry, F7 adds to its map; the message names the missing fields.
    a.select(Path::parse("targets.nas1").unwrap());
    a.focus = Focus::Tree;
    key(&mut a, KeyCode::Insert);
    typ(&mut a, "nas2");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.message.as_ref().unwrap().text, "new Storage target 'nas2': fill in 1 required field");
    ctrl(&mut a, 'z');
    ctrl(&mut a, 'z');
    assert!(!a.modified());
}

#[test]
fn ref_keys_are_chosen_from_the_target_map() {
    let mut a = app();
    a.select(Path::parse("jobs.documents.targets").unwrap());
    a.fkey(7);
    let Mode::Choose { options, purpose: ChooseFor::NewKey(_), .. } = &a.mode else { panic!("{:?}", a.mode) };
    assert_eq!(options.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["nas1"]);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.selected.to_string(), "jobs.documents.targets.nas1");
    assert!(a.cfg.to_text().contains("[jobs.documents.targets.nas1]\nusername = \"\"\n"));
    // photos already uses nas1: nothing left to choose.
    a.select(Path::parse("jobs.photos.targets").unwrap());
    a.fkey(7);
    assert_eq!(a.mode, Mode::Normal);
    assert!(a.message.as_ref().unwrap().text.contains("no free key"));
}

#[test]
fn deleting_asks_and_warns_about_references() {
    let mut a = app();
    a.select(Path::parse("targets.nas1").unwrap());
    a.focus = Focus::Tree;
    assert!(fkey_bar(&mut a).contains("8Delete"));
    a.fkey(8);
    assert_eq!(a.mode, Mode::Confirm(Confirm::Delete(Path::parse("targets.nas1").unwrap())));
    let screen = render(&mut a);
    assert!(screen.contains("Delete targets.nas1 with everything in it?"), "{screen}");
    assert!(screen.contains("Still used by jobs.photos.targets.nas1."), "{screen}");
    key(&mut a, KeyCode::Char('n'));
    assert!(a.value(&Path::parse("targets.nas1.host").unwrap()).is_some());
    a.fkey(8);
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(a.selected.to_string(), "targets");
    assert!(a.message.as_ref().unwrap().text.contains("1 reference to it now points nowhere"));
    assert!(a.counts().0 >= 1);

    // In the form of a map: the entry under the cursor.
    goto(&mut a, "jobs", "documents");
    a.fkey(8);
    assert_eq!(a.mode, Mode::Confirm(Confirm::Delete(Path::parse("jobs.documents").unwrap())));
    key(&mut a, KeyCode::Enter);
    assert!(!a.cfg.to_text().contains("[jobs.documents]"));
}

#[test]
fn schema_can_forbid_new_and_deleted_entries() {
    let fixed = EXAMPLE.replace("jobs         : map(Job, Name) = {}", "jobs         : map(Job, Name) = {} readonly");
    let mut a = ConfApp::new(PathBuf::from("t.toml"), ConfigFile::parse(&fixed).unwrap());
    assert!(a.cfg.schema_errors.is_empty(), "{:?}", a.cfg.schema_errors);
    a.select(Path::parse("jobs.photos").unwrap());
    let bar = fkey_bar(&mut a);
    assert!(!bar.contains("7New") && !bar.contains("8Delete"), "{bar}");
    a.fkey(7);
    assert_eq!(a.message.as_ref().unwrap().text, "jobs is read-only: no new entries");
    a.fkey(8);
    assert_eq!(a.mode, Mode::Normal);

    let limited = EXAMPLE.replace("jobs         : map(Job, Name) = {}", "jobs         : map(Job, Name) length ..2 = {}");
    let mut a = ConfApp::new(PathBuf::from("t.toml"), ConfigFile::parse(&limited).unwrap());
    a.select(Path::parse("jobs").unwrap());
    a.fkey(7);
    assert_eq!(a.message.as_ref().unwrap().text, "jobs allows ..2 entries");
    // Other maps are not affected.
    a.select(Path::parse("accounts").unwrap());
    a.fkey(7);
    assert!(matches!(a.mode, Mode::NewKey { .. }));
}

#[test]
fn open_another_file_after_settling_changes() {
    let dir = std::env::temp_dir().join(format!("hardconf-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.toml"), EXAMPLE).unwrap();
    std::fs::write(dir.join("b.toml"), "x = 1\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "-").unwrap();
    let mut a = ConfApp::open(&dir.join("a.toml")).unwrap();
    goto(&mut a, "jobs.photos", "max_per_day");
    typ(&mut a, "7");
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'e');
    assert_eq!(a.mode, Mode::Confirm(Confirm::Unsaved(Then::Open)));
    key(&mut a, KeyCode::Char('d'));
    let names: Vec<String> = a.file_dialog.as_ref().unwrap().shown().iter().map(|e| e.name.clone()).collect();
    assert_eq!(names, ["..", "sub", "a.toml", "b.toml"], "folders and TOML files");
    assert!(render(&mut a).contains("open file"));
    typ(&mut a, "b.t");
    key(&mut a, KeyCode::Enter);
    assert!(a.file_dialog.is_none());
    assert!(a.path.ends_with("b.toml"));
    assert!(a.cfg.schema.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("a.toml")).unwrap(), EXAMPLE, "discarded, not written");
    // A file hardconf cannot read: the open one stays.
    std::fs::write(dir.join("bad.toml"), "a = [").unwrap();
    a.fkey(5);
    typ(&mut a, "bad");
    key(&mut a, KeyCode::Enter);
    assert!(a.path.ends_with("b.toml"));
    assert!(a.message.as_ref().is_some_and(|m| m.error && m.text.contains("cannot open")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comments_are_edited_with_f6() {
    let mut a = app();
    // A table: the comment at its end is offered and moves above the header.
    a.select(Path::parse("targets.nas1").unwrap());
    a.focus = Focus::Tree;
    assert!(fkey_bar(&mut a).contains("6Comment"));
    a.fkey(6);
    let Mode::Comment { buf, .. } = &a.mode else { panic!("{:?}", a.mode) };
    assert!(buf.starts_with("No certificate accepted yet") && buf.ends_with("Used by: photos"));
    ctrl(&mut a, 'u');
    typ(&mut a, "NAS in the basement");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "UPS until 2027");
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.mode, Mode::Normal);
    let text = a.cfg.to_text();
    assert!(text.contains("\n# NAS in the basement\n# UPS until 2027\n[targets.nas1]\n"), "{text}");
    assert!(!text.contains("Used by"));
    assert!(text.contains("tls_fingerprint = \"\"\n\n[accounts.archive]"));
    assert_eq!(a.comments[&Path::parse("targets.nas1").unwrap()], "NAS in the basement\nUPS until 2027");
    ctrl(&mut a, 'z');
    assert_eq!(a.cfg.to_text(), EXAMPLE, "undo restores the comments");

    // A field.
    goto(&mut a, "targets.nas1", "host");
    a.fkey(6);
    typ(&mut a, "Name in the LAN");
    ctrl(&mut a, 's');
    assert!(a.cfg.to_text().contains("api_port  = 8443\ntls"), "other keys untouched");
    assert!(a.cfg.to_text().contains("id        = 3\n# Name in the LAN\nhost      = \"nas.example.lan\"\n"));
    assert!(render(&mut a).contains("Name in the LAN"), "shown in the doc panel");
    // Empty text removes it.
    a.fkey(6);
    ctrl(&mut a, 'u');
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.cfg.to_text(), EXAMPLE);

    // A field that is not in the file has no line to put a comment on.
    goto(&mut a, "targets.nas1", "tls_name");
    assert!(!fkey_bar(&mut a).contains("6Comment"));
    a.fkey(6);
    assert_eq!(a.mode, Mode::Normal);
    assert!(a.message.as_ref().unwrap().text.contains("not in the file"));

    // The root: the comment at the top of the file.
    a.select(Path::root());
    a.focus = Focus::Tree;
    a.fkey(6);
    assert!(matches!(&a.mode, Mode::Comment { buf, .. } if buf.starts_with("Backup configuration – example")));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.mode, Mode::Normal);
}

#[test]
fn multiline_notes_are_a_text_area() {
    let mut a = app();
    goto(&mut a, "jobs.photos", "notes");
    let screen = render(&mut a);
    assert!(screen.contains("notes                 Uploads the photo library"), "{screen}");
    assert!(screen.contains("Notes                 Questions: see the wiki page"), "hint below the label: {screen}");

    goto(&mut a, "jobs.documents", "notes");
    typ(&mut a, "line 1");
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "line 2");
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.value(&Path::parse("jobs.documents.notes").unwrap()).and_then(|v| v.as_str()), Some("line 1\nline 2"));
    let screen = render(&mut a);
    assert!(screen.contains("notes                 line 1") && screen.contains("line 2"), "{screen}");
    // Rows below the text area are still reachable and clickable.
    key(&mut a, KeyCode::Down);
    assert_eq!(a.current_row().unwrap().label, "token");
}

const GENERAL: &str = include_str!("../examples/general.toml");

fn general() -> ConfApp {
    ConfApp::new(PathBuf::from("general.toml"), ConfigFile::parse(GENERAL).unwrap())
}

/// Takes the option `label` of the open choice list.
fn choose(a: &mut ConfApp, label: &str) {
    let Mode::Choose { options, cursor, .. } = &mut a.mode else { panic!("no choice list: {:?}", a.mode) };
    *cursor = options.iter().position(|(v, _)| v == label).unwrap_or_else(|| panic!("no option {label}"));
    key(a, KeyCode::Enter);
}

#[test]
fn general_toml_opens_as_a_tree() {
    let mut a = general();
    assert!(a.cfg.schema.is_none());
    assert_eq!(a.cfg.to_text(), GENERAL, "unchanged save: same bytes");
    a.focus = Focus::Form;
    let screen = render(&mut a);
    assert!(screen.lines().next().unwrap().ends_with(" plain TOML"), "{screen}");
    for node in ["server", "database", "members", "rooms"] {
        assert!(screen.contains(&format!("  {node}")), "{node}: {screen}");
    }
    // The multi-line string is a text area.
    assert!(screen.contains("motd      Welcome to the clubhouse!"), "{screen}");
    assert!(screen.contains("          Please switch off the lights and heating when you leave."), "{screen}");
    assert!(a.findings(&Path::root()).is_empty() && a.counts() == (0, 0), "nothing to check");
}

#[test]
fn multiline_strings_without_schema() {
    let mut a = general();
    goto(&mut a, "", "motd");
    key(&mut a, KeyCode::Enter);
    assert!(matches!(&a.mode, Mode::Edit { multiline: true, buf, .. } if buf.contains('\n')));
    key(&mut a, KeyCode::End);
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "Thanks!");
    key(&mut a, KeyCode::Tab);
    assert!(a.cfg.to_text().contains("motd = \"\"\"\nWelcome to the clubhouse!\nPlease switch off the lights and heating when you leave.\nThanks!\"\"\"\n"), "{}", a.cfg.to_text());
    // A literal '''…''' string is multi-line too.
    goto(&mut a, "rooms.kitchen", "note");
    key(&mut a, KeyCode::Enter);
    assert!(matches!(&a.mode, Mode::Edit { multiline: true, .. }));
    key(&mut a, KeyCode::Esc);
    assert!(render(&mut a).contains("Empty the fridge on Fridays."));
}

#[test]
fn new_keys_tables_and_elements_without_schema() {
    let mut a = general();
    // A text at the root: name, kind, value.
    a.select(Path::root());
    a.focus = Focus::Tree;
    assert!(fkey_bar(&mut a).contains("7New"));
    a.fkey(7);
    assert!(matches!(&a.mode, Mode::NewKey { generic: true, .. }));
    typ(&mut a, "title");
    key(&mut a, KeyCode::Enter);
    assert!(a.message.as_ref().unwrap().text.contains("exists already"));
    ctrl(&mut a, 'u');
    typ(&mut a, "contact");
    key(&mut a, KeyCode::Enter);
    let Mode::Choose { options, .. } = &a.mode else { panic!("{:?}", a.mode) };
    assert_eq!(options.len(), 9);
    choose(&mut a, "text");
    assert!(matches!(&a.mode, Mode::Edit { .. }), "straight to the value");
    typ(&mut a, "info@example.org");
    key(&mut a, KeyCode::Enter);
    let text = a.cfg.to_text();
    assert!(text.find("contact = \"info@example.org\"").unwrap() < text.find("[server]").unwrap(), "{text}");

    // A list in a table: its first element is typed right away.
    a.select(Path::parse("server").unwrap());
    a.fkey(7);
    typ(&mut a, "ports");
    key(&mut a, KeyCode::Enter);
    choose(&mut a, "list");
    typ(&mut a, "80");
    key(&mut a, KeyCode::Enter);
    assert!(a.cfg.to_text().contains("ports = [80]\n"));

    // A table, and a date in it.
    a.select(Path::root());
    a.fkey(7);
    typ(&mut a, "events");
    key(&mut a, KeyCode::Enter);
    choose(&mut a, "table");
    assert_eq!(a.selected.to_string(), "events");
    a.fkey(7);
    typ(&mut a, "next");
    key(&mut a, KeyCode::Enter);
    choose(&mut a, "date");
    ctrl(&mut a, 'u');
    typ(&mut a, "24.12.2026");
    key(&mut a, KeyCode::Enter);
    assert!(a.cfg.to_text().ends_with("\n[events]\nnext = 2026-12-24\n"), "{}", a.cfg.to_text());

    // A new element of a [[list]] goes after the last one.
    a.select(Path::parse("members").unwrap());
    a.focus = Focus::Tree;
    a.fkey(7);
    assert_eq!(a.selected.to_string(), "members[2]");
    a.fkey(7);
    typ(&mut a, "name");
    key(&mut a, KeyCode::Enter);
    choose(&mut a, "text");
    typ(&mut a, "Clara");
    key(&mut a, KeyCode::Enter);
    let text = a.cfg.to_text();
    let clara = text.find("[[members]]\nname = \"Clara\"").unwrap_or_else(|| panic!("{text}"));
    assert!(text.find("Ben Okafor").unwrap() < clara && clara < text.find("[rooms.hall]").unwrap());
}

#[test]
fn deleting_and_retyping_without_schema() {
    let mut a = general();
    // A table with its comment-free neighbours intact.
    a.select(Path::parse("rooms.kitchen").unwrap());
    a.focus = Focus::Tree;
    a.fkey(8);
    assert_eq!(a.mode, Mode::Confirm(Confirm::Delete(Path::parse("rooms.kitchen").unwrap())));
    key(&mut a, KeyCode::Char('y'));
    assert!(a.cfg.to_text().ends_with("equipment = [\"Projector\", \"Microphone\"]\n"), "{}", a.cfg.to_text());
    // An element of a [[list]].
    a.select(Path::parse("members[0]").unwrap());
    a.fkey(8);
    key(&mut a, KeyCode::Char('y'));
    assert!(!a.cfg.to_text().contains("Anna Berger") && a.cfg.to_text().contains("Ben Okafor"));
    // An inline table from the form, and a plain value with F8.
    goto(&mut a, "database", "pool");
    a.fkey(8);
    key(&mut a, KeyCode::Char('y'));
    assert!(!a.cfg.to_text().contains("pool"));
    goto(&mut a, "server", "timeout");
    a.fkey(8);
    assert!(!a.cfg.to_text().contains("timeout"));

    // F9: the value converts where it can.
    goto(&mut a, "server", "port");
    assert!(fkey_bar(&mut a).contains("9Type"));
    a.fkey(9);
    choose(&mut a, "text");
    assert!(a.cfg.to_text().contains("port    = \"8443\"\n"), "spacing kept: {}", a.cfg.to_text());
    a.fkey(9);
    choose(&mut a, "integer");
    assert!(a.cfg.to_text().contains("port    = 8443\n"));
    goto(&mut a, "server", "host");
    a.fkey(9);
    choose(&mut a, "integer");
    assert_eq!(value(&a, "server.host"), "0");
    assert!(a.message.as_ref().is_some_and(|m| m.error && m.text.contains("is no integer")));
    goto(&mut a, "", "title");
    a.fkey(9);
    choose(&mut a, "list");
    assert_eq!(value(&a, "title"), "[\"Clubhouse Sunnyside\"]");
    a.fkey(9);
    choose(&mut a, "multi-line text");
    assert!(a.cfg.to_text().contains("title   = \"\"\"\nClubhouse Sunnyside\"\"\""), "{}", a.cfg.to_text());

    // With a schema, the schema decides.
    let mut t = app();
    goto(&mut t, "targets.nas1", "ssh_port");
    assert!(!fkey_bar(&mut t).contains("9Type"));
    t.fkey(9);
    assert_eq!(t.message.as_ref().unwrap().text, "the schema sets the type of ssh_port");
}

#[test]
fn format_at_first_glance() {
    let top = |a: &mut ConfApp| render(a).lines().next().unwrap().to_string();
    let mut a = app();
    assert!(top(&mut a).contains("backup.toml   ✓ hardconf v1"), "{}", top(&mut a));
    assert!(render(&mut a).contains("format   hardconf schema v1 · 12 types · 6 structs"));
    let mut g = general();
    assert!(top(&mut g).ends_with("general.toml   plain TOML"), "{}", top(&mut g));
    assert!(render(&mut g).contains("format   plain TOML · no #%hardconf block"));
    // A schema block hardconf cannot use, and one with a bad line.
    for text in [EXAMPLE.replace("#%hardconf 1", "#%hardconf 2"), EXAMPLE.replace("#%  Port      = int range 1..65535", "#%  Port      = int range x..")] {
        let mut b = ConfApp::new(PathBuf::from("b.toml"), ConfigFile::parse(&text).unwrap());
        assert!(top(&mut b).contains("✗ hardconf schema problems"), "{}", top(&mut b));
        assert!(render(&mut b).contains("#%hardconf block with problems"));
    }

    // The file dialog marks files with a schema.
    let dir = std::env::temp_dir().join(format!("hardconf-tag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.toml"), EXAMPLE).unwrap();
    std::fs::write(dir.join("b.toml"), GENERAL).unwrap();
    let mut a = ConfApp::open(&dir.join("b.toml")).unwrap();
    a.fkey(5);
    let tags: Vec<(String, Option<&str>)> = a.file_dialog.as_ref().unwrap().shown().iter().map(|e| (e.name.clone(), e.tag)).collect();
    assert_eq!(tags, [("..".into(), None), ("a.toml".into(), Some("hardconf")), ("b.toml".into(), None)]);
    let screen = render(&mut a);
    let line = screen.lines().find(|l| l.contains("a.toml")).unwrap();
    assert!(line.contains("hardconf"), "{line}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_and_paste() {
    let mut a = app();
    goto(&mut a, "targets.nas1", "host");
    ctrl(&mut a, 'c');
    assert_eq!(a.clipboard.as_deref(), Some("nas.example.lan"));
    assert_eq!(a.to_system_clipboard.as_deref(), Some("nas.example.lan"), "also for the terminal (OSC 52)");
    goto(&mut a, "accounts.archive", "host");
    ctrl(&mut a, 'v');
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(value(&a, "accounts.archive.host"), "\"nas.example.lan\"");

    // A paste from the terminal (Cmd+V) outside editing sets the value.
    goto(&mut a, "jobs.photos", "max_per_day");
    a.on_paste("55\n");
    assert_eq!(value(&a, "jobs.photos.max_per_day"), "55");
    // What does not fit stays in the edit field with the error.
    a.on_paste("viele");
    assert!(matches!(&a.mode, Mode::Edit { buf, .. } if buf == "viele"));
    assert!(a.message.as_ref().is_some_and(|m| m.error));
    key(&mut a, KeyCode::Esc);

    // While editing: Ctrl+C copies the field, Ctrl+V inserts.
    goto(&mut a, "targets.nas1", "host");
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'c');
    key(&mut a, KeyCode::End);
    ctrl(&mut a, 'v');
    assert!(matches!(&a.mode, Mode::Edit { buf, .. } if buf == "nas.example.lannas.example.lan"));
    key(&mut a, KeyCode::Esc);
    // In the tree: the key of the node; a table row has nothing to copy.
    a.select(Path::parse("jobs.documents").unwrap());
    a.focus = Focus::Tree;
    ctrl(&mut a, 'c');
    assert_eq!(a.clipboard.as_deref(), Some("documents"));
    goto(&mut a, "jobs.photos", "include");
    ctrl(&mut a, 'v');
    assert!(a.message.as_ref().unwrap().text.contains("not a table"));
}

#[test]
fn hints_have_their_own_colour() {
    let mut a = app();
    goto(&mut a, "targets.nas1", "host");
    let mut term = Terminal::new(TestBackend::new(110, 32)).unwrap();
    term.draw(|f| ui::draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer();
    let find = |text: &str| -> (u16, u16) {
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(i) = line.find(text) {
                return (line[..i].chars().count() as u16, y);
            }
        }
        panic!("{text} not on screen");
    };
    let (x, y) = find("Name or address of the server");
    let hint = &buf[(x, y)];
    assert_eq!(hint.fg, ratatui::style::Color::Indexed(109));
    assert!(hint.modifier.contains(ratatui::style::Modifier::ITALIC));
    let (x, y) = find("nas.example.lan");
    assert_ne!(buf[(x, y)].fg, ratatui::style::Color::Indexed(109), "values look different");
}

#[test]
fn right_and_left_switch_between_tree_and_form() {
    let mut a = app();
    for node in ["", "targets", "jobs.photos", "targets.nas1"] {
        a.select(if node.is_empty() { Path::root() } else { Path::parse(node).unwrap() });
        a.focus = Focus::Tree;
        key(&mut a, KeyCode::Right);
        assert_eq!((a.focus, a.selected.to_string()), (Focus::Form, node.to_string()), "→ on {node:?}");
        key(&mut a, KeyCode::Left);
        assert_eq!(a.focus, Focus::Tree, "← back from the form");
    }
    // Opening and closing stays in the tree.
    let p = Path::parse("jobs.photos").unwrap();
    a.select(p.clone());
    a.focus = Focus::Tree;
    key(&mut a, KeyCode::Char('+'));
    assert!(a.expanded.contains(&p) && a.focus == Focus::Tree);
    key(&mut a, KeyCode::Char('-'));
    assert!(!a.expanded.contains(&p));
    key(&mut a, KeyCode::Char(' '));
    assert!(a.expanded.contains(&p));
    key(&mut a, KeyCode::Left);
    assert!(!a.expanded.contains(&p), "← closes");
    key(&mut a, KeyCode::Left);
    assert_eq!(a.selected.to_string(), "jobs", "← again: the parent");
}
