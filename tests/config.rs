use hardcalc::config::{ConfigFile, FieldType, Path, Severity};

const EXAMPLE: &str = include_str!("../examples/backup.toml");

fn findings(cfg: &ConfigFile) -> Vec<String> {
    cfg.validate().iter().map(|d| d.to_string()).collect()
}

#[test]
fn example_schema_is_clean() {
    let cfg = ConfigFile::parse(EXAMPLE).unwrap();
    assert!(cfg.schema_errors.is_empty(), "{:#?}", cfg.schema_errors);
    let schema = cfg.schema.as_ref().unwrap();
    assert_eq!(schema.root.title.as_deref(), Some("Backup configuration"));
    let names: Vec<&str> = schema.structs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Rules", "Patterns", "Target", "Account", "Job", "TargetAccess"]);
    let agent = schema.struct_def("Job").unwrap();
    let FieldType::Map { key, .. } = &agent.field("targets").unwrap().ty else { panic!("targets is a map") };
    assert_eq!(schema.key_label(key), "Storage target");
    let FieldType::Map { key, .. } = &schema.root.field("jobs").unwrap().ty else { panic!("jobs is a map") };
    assert_eq!(schema.key_label(key), "Name");
    assert_eq!(findings(&cfg), Vec::<String>::new());
}

#[test]
fn unchanged_save_is_byte_identical() {
    let cfg = ConfigFile::parse(EXAMPLE).unwrap();
    assert_eq!(cfg.to_text(), EXAMPLE);
}

#[test]
fn edit_changes_only_that_value() {
    let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
    cfg.doc["targets"]["nas1"]["ssh_port"] = toml_edit::value(1445);
    let saved = cfg.to_text();
    let changed: Vec<(&str, &str)> = EXAMPLE.lines().zip(saved.lines()).filter(|(a, b)| a != b).collect();
    assert_eq!(changed, [("ssh_port  = 22", "ssh_port  = 1445")]);
    assert_eq!(EXAMPLE.lines().count(), saved.lines().count());
}

#[test]
fn diagnostics_are_read_and_dropped_on_save() {
    let text = format!(
        "#%error jobs.photos.notify \"not a valid email address\"\n#%error \"two jobs with the same id\"\n{EXAMPLE}#%warning targets.nas1 \"no certificate yet\"\n"
    );
    let cfg = ConfigFile::parse(&text).unwrap();
    assert!(cfg.schema_errors.is_empty(), "{:#?}", cfg.schema_errors);
    let d: Vec<String> = cfg.file_diagnostics.iter().map(|d| format!("{} {}", d.line, d.diagnostic)).collect();
    assert_eq!(
        d,
        [
            "1 error: jobs.photos.notify: not a valid email address",
            "2 error: two jobs with the same id",
            &format!("{} warning: targets.nas1: no certificate yet", EXAMPLE.lines().count() + 3),
        ]
    );
    assert_eq!(cfg.to_text(), EXAMPLE);
}

#[test]
fn without_header_hash_percent_is_a_comment() {
    let text = "#%error \"x\"\na = 1\n#%types\n";
    let cfg = ConfigFile::parse(text).unwrap();
    assert!(cfg.schema.is_none() && cfg.file_diagnostics.is_empty());
    assert_eq!(cfg.to_text(), text);
}

#[test]
fn reports_data_errors() {
    let data = r#"
revision = "r-1"
extra = 1

[targets.nas1]
host     = "nas.example.lan"
ssh_port = 0
tls      = "verify"
tls_ca   = "-----BEGIN"

[accounts.archive]
host = "s3.example.com"
mirror_host = "s3 example com"

[jobs.Bot]
transport   = "ftp"
allowed_ips = ["10.0.0.0/8", "10.0.0.0/40"]
note = """
#%error "inside a string"
"""

[jobs.Bot.targets.nas2]
username = "x"
read     = "yes"
"#;
    let schema = &EXAMPLE[EXAMPLE.find("#%hardconf").unwrap()..];
    let cfg = ConfigFile::parse(&format!("{data}\n{schema}")).unwrap();
    assert!(cfg.file_diagnostics.is_empty());
    let all = cfg.validate();
    let got: Vec<String> = all.iter().map(|d| d.to_string()).collect();
    assert_eq!(
        got,
        [
            "error: targets.nas1.ssh_port: 0 outside 1..65535",
            "note: targets.nas1.tls_ca: has no effect, only used when tls = ca",
            "error: accounts.archive.mirror_host: invalid host name: s3 example com",
            "error: accounts.archive.username: missing required field",
            "error: jobs.Bot: bad key: 'Bot' does not match ^[a-z][a-z0-9_-]*$",
            "error: jobs.Bot.transport: 'ftp' is not one of https, http",
            "error: jobs.Bot.allowed_ips[1]: invalid network (a.b.c.d/n): 10.0.0.0/40",
            "error: jobs.Bot.targets.nas2: 'nas2' is not an entry of targets",
            "error: jobs.Bot.targets.nas2.read: expected bool, found text",
            "warning: jobs.Bot.note: not in schema",
            "warning: extra: not in schema",
        ],
        "{got:#?}"
    );
    assert!(all.iter().filter(|d| d.severity == Severity::Error).count() == 8);
    assert_eq!(all[0].path, Path::parse("targets.nas1.ssh_port").unwrap());
}

mod entries {
    use super::EXAMPLE;
    use hardcalc::config::edit::{add_entry, key_problem, new_entry, remove_entry};
    use hardcalc::config::{ConfigFile, FieldType, Path};

    fn p(s: &str) -> Path {
        Path::parse(s).unwrap()
    }

    fn add(cfg: &mut ConfigFile, map: &str, key: &str) {
        let schema = cfg.schema.as_ref().unwrap();
        let field = schema.field_at(&p(map)).or_else(|| {
            // A map inside a map entry: jobs.<name>.accounts
            schema.struct_def("Job").unwrap().field(map.rsplit('.').next().unwrap())
        });
        let FieldType::Map { value, .. } = &field.unwrap().ty else { panic!("{map} is a map") };
        let entry = new_entry(schema, value);
        add_entry(&mut cfg.doc, &p(map), key, entry).unwrap();
    }

    #[test]
    fn new_entries_hold_required_fields_and_sit_after_the_map() {
        let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
        add(&mut cfg, "targets", "nas2");
        add(&mut cfg, "jobs", "new");
        add(&mut cfg, "jobs.documents.accounts", "private");
        let text = cfg.to_text();
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} missing:\n{text}"));
        assert!(at("[targets.nas1]") < at("[targets.nas2]\nhost = \"\"\n") && at("[targets.nas2]") < at("[accounts.archive]"));
        // The comment at the end of nas1 stays with nas1, above the new table.
        assert!(at("# Used by: photos\n") < at("[targets.nas2]"));
        assert!(at("[jobs.documents]") < at("[jobs.new]\n") && at("[jobs.new]") < at("#%hardconf"));
        assert!(at("[jobs.new]") < at("[jobs.documents.accounts.private]\nhost = \"\"\nmirror_host = \"\"\nusername = \"\"\n"));
        assert!(text.ends_with(&EXAMPLE[EXAMPLE.find("\n#%hardconf").unwrap()..]), "schema untouched at the end");
        // Required fields are empty: the check asks for them.
        let errors: Vec<String> = cfg.validate().iter().map(|d| d.to_string()).collect();
        assert!(errors.contains(&"error: targets.nas2.host: invalid host name: ".to_string()), "{errors:#?}");
    }

    #[test]
    fn keys_are_checked() {
        let cfg = ConfigFile::parse(EXAMPLE).unwrap();
        let s = cfg.schema.as_ref().unwrap();
        let root = cfg.doc.as_table();
        let FieldType::Map { key, .. } = &s.root.field("jobs").unwrap().ty else { panic!() };
        assert_eq!(key_problem(s, root, &p("jobs"), key, "new"), None);
        assert!(key_problem(s, root, &p("jobs"), key, "photos").unwrap().contains("exists"));
        assert!(key_problem(s, root, &p("jobs"), key, "New").unwrap().contains("does not match"));
        let FieldType::Map { key, .. } = &s.struct_def("Job").unwrap().field("targets").unwrap().ty else { panic!() };
        assert_eq!(key_problem(s, root, &p("jobs.documents.targets"), key, "nas1"), None);
        assert!(key_problem(s, root, &p("jobs.documents.targets"), key, "nas9").unwrap().contains("not an entry of targets"));
    }

    #[test]
    fn removing_keeps_the_comments_of_the_neighbours() {
        let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
        remove_entry(&mut cfg.doc, &p("accounts.archive")).unwrap();
        let start = EXAMPLE.find("[accounts.archive]").unwrap();
        let end = EXAMPLE.find("[jobs.photos]").unwrap();
        assert_eq!(cfg.to_text(), format!("{}{}", &EXAMPLE[..start], &EXAMPLE[end..]));

        // The comment at the end of nas1 goes with it.
        let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
        assert_eq!(cfg.references(&p("targets"), "nas1"), [p("jobs.photos.targets.nas1")]);
        remove_entry(&mut cfg.doc, &p("targets.nas1")).unwrap();
        let start = EXAMPLE.find("[targets.nas1]").unwrap();
        let end = EXAMPLE.find("[accounts.archive]").unwrap();
        assert_eq!(cfg.to_text(), format!("{}{}", &EXAMPLE[..start], &EXAMPLE[end..]));
        assert!(cfg.validate().iter().any(|d| d.message == "'nas1' is not an entry of targets"));

        // The last table: the schema block stays.
        let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
        remove_entry(&mut cfg.doc, &p("jobs.documents")).unwrap();
        let start = EXAMPLE.find("[jobs.documents]").unwrap();
        let end = EXAMPLE.find("#%hardconf").unwrap();
        assert_eq!(cfg.to_text(), format!("{}{}", &EXAMPLE[..start], &EXAMPLE[end..]));

        // An entry with sub-tables.
        let mut cfg = ConfigFile::parse(EXAMPLE).unwrap();
        remove_entry(&mut cfg.doc, &p("jobs.photos")).unwrap();
        let start = EXAMPLE.find("[jobs.photos]").unwrap();
        let end = EXAMPLE.find("[jobs.documents]").unwrap();
        assert_eq!(cfg.to_text(), format!("{}{}", &EXAMPLE[..start], &EXAMPLE[end..]));
    }

    #[test]
    fn map_length_limits_entries() {
        let text = EXAMPLE.replace("jobs         : map(Job, Name) = {}", "jobs         : map(Job, Name) length ..1 = {}");
        let cfg = ConfigFile::parse(&text).unwrap();
        assert!(cfg.schema_errors.is_empty(), "{:?}", cfg.schema_errors);
        let d: Vec<String> = cfg.validate().iter().map(|d| d.to_string()).collect();
        assert_eq!(d, ["error: jobs: 2 entries, allowed ..1"]);
        // A default that breaks the limit is a schema error.
        let bad = ConfigFile::parse(&EXAMPLE.replace("jobs         : map(Job, Name) = {}", "jobs         : map(Job, Name) length 1.. = {}")).unwrap();
        assert_eq!(bad.schema_errors[0].message, "jobs: default value: 0 entries, allowed 1..");
    }
}
