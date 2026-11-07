use std::process::ExitCode;

const USAGE: &str = "\
usage: hardconf <file.toml>    edit a TOML file; its #% schema block describes the fields
       hardconf                choose the file in a file dialog
       hardconf --version

Quitting without saving leaves the file untouched, so hardconf can serve as
$EDITOR for programs that detect a cancelled edit that way.";

fn main() -> ExitCode {
    hardcalc::locale::init_from_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("hardconf {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [] => match hardcalc::tui::config::run(None) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("hardconf: {e}");
                ExitCode::FAILURE
            }
        },
        [path] if !path.starts_with('-') => match hardcalc::tui::config::run(Some(std::path::Path::new(path))) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("hardconf: {path}: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
