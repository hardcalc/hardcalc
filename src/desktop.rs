//! Start menu entry and `.hcmd` file association: `hardcalc desktop install`
//! and `hardcalc desktop uninstall`.
//!
//! Everything goes into the user's own profile, no administrator rights are
//! needed, and the installers (Homebrew, shell, PowerShell, cargo) stay out of
//! it – the user runs the command once.
//!
//! - **Linux**: a `.desktop` file with `Terminal=true`, a shared-mime-info type
//!   `text/x-hardcalc` for `*.hcmd` and the icon in the hicolor theme, below
//!   `$XDG_DATA_HOME` (`~/.local/share`).
//! - **Windows**: a shortcut in the start menu and the ProgID `hardcalc.hcmd`
//!   below `HKCU\Software\Classes`, written by a PowerShell script.
//! - **macOS**: Finder hands files to applications via Apple Events, not on
//!   the command line, so a small AppleScript app `~/Applications/hardcalc.app`
//!   opens Terminal and runs hardcalc there. It declares the type
//!   `org.hardcalc.hcmd`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const ICON_SVG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/hardcalc.svg"));
const ICON_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/hardcalc.png"));
const ICON_ICO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/hardcalc.ico"));
const ICON_ICNS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/hardcalc.icns"));

/// Shared-mime-info type of `.hcmd` files (Linux).
pub const MIME_TYPE: &str = "text/x-hardcalc";
/// Uniform type identifier of `.hcmd` files (macOS).
pub const UTI: &str = "org.hardcalc.hcmd";
/// Bundle identifier of the macOS launcher app.
pub const BUNDLE_ID: &str = "org.hardcalc.launcher";
/// ProgID of `.hcmd` files (Windows).
pub const PROG_ID: &str = "hardcalc.hcmd";

/// `hardcalc desktop install`: returns one line per thing done.
pub fn install() -> Result<Vec<String>, String> {
    let exe = stable_exe()?;
    let mut done = Vec::new();
    if cfg!(target_os = "macos") {
        macos_install(&exe, &mut done)?;
    } else if cfg!(windows) {
        windows_run("install", &exe, &mut done)?;
    } else {
        linux_install(&exe, &mut done)?;
    }
    if exe.components().any(|c| c.as_os_str() == "Cellar") {
        done.push(format!(
            "note: {} is inside the Homebrew Cellar and changes with every upgrade – \
             run `hardcalc desktop install` again after `brew upgrade`",
            exe.display()
        ));
    }
    Ok(done)
}

/// `hardcalc desktop uninstall`: returns one line per thing removed.
pub fn uninstall() -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    if cfg!(target_os = "macos") {
        macos_uninstall(&mut done)?;
    } else if cfg!(windows) {
        windows_run("uninstall", Path::new(""), &mut done)?;
    } else {
        linux_uninstall(&mut done)?;
    }
    if done.is_empty() {
        done.push("nothing to remove".into());
    }
    Ok(done)
}

/// The path under which this program is reached: the entry in `PATH` that
/// leads to the running binary (e.g. `/opt/homebrew/bin/hardcalc` instead of
/// the versioned Cellar path behind it), else the running binary itself.
pub fn stable_exe() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("cannot determine the path of hardcalc: {e}"))?;
    let real = fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
    let name = exe.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "hardcalc".into());
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths).filter(|d| d.is_absolute()) {
            let candidate = dir.join(&name);
            if candidate.is_file() && fs::canonicalize(&candidate).ok().as_deref() == Some(real.as_path()) {
                return Ok(candidate);
            }
        }
    }
    Ok(exe)
}

// ---------------------------------------------------------------- helpers

fn home() -> Result<PathBuf, String> {
    env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).ok_or_else(|| "HOME is not set".to_string())
}

fn write_file(path: &Path, data: &[u8], done: &mut Vec<String>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))?;
    done.push(format!("wrote {}", path.display()));
    Ok(())
}

fn remove_file(path: &Path, done: &mut Vec<String>) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => {
            done.push(format!("removed {}", path.display()));
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Runs a program that must succeed.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{program} {}: {}", args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Runs an optional helper; a missing program or a failure becomes a note.
fn run_optional(program: &str, args: &[&str], done: &mut Vec<String>) {
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => done.push(format!("ran {program}")),
        Ok(out) => done.push(format!("note: {program} failed: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => done.push(format!("note: {program} not found, skipped")),
        Err(e) => done.push(format!("note: {program}: {e}")),
    }
}

// ---------------------------------------------------------------- Linux

fn data_home() -> Result<PathBuf, String> {
    match env::var_os("XDG_DATA_HOME") {
        Some(d) if Path::new(&d).is_absolute() => Ok(PathBuf::from(d)),
        _ => Ok(home()?.join(".local/share")),
    }
}

struct LinuxFiles {
    desktop: PathBuf,
    mime: PathBuf,
    png: PathBuf,
    svg: PathBuf,
}

fn linux_files(data: &Path) -> LinuxFiles {
    LinuxFiles {
        desktop: data.join("applications/hardcalc.desktop"),
        mime: data.join("mime/packages/hardcalc.xml"),
        png: data.join("icons/hicolor/256x256/apps/hardcalc.png"),
        svg: data.join("icons/hicolor/scalable/apps/hardcalc.svg"),
    }
}

fn linux_install(exe: &Path, done: &mut Vec<String>) -> Result<(), String> {
    let data = data_home()?;
    let f = linux_files(&data);
    write_file(&f.png, ICON_PNG, done)?;
    write_file(&f.svg, ICON_SVG, done)?;
    write_file(&f.mime, mime_xml().as_bytes(), done)?;
    write_file(&f.desktop, desktop_entry(exe).as_bytes(), done)?;
    linux_refresh(&data, done);
    run_optional("xdg-mime", &["default", "hardcalc.desktop", MIME_TYPE], done);
    Ok(())
}

fn linux_uninstall(done: &mut Vec<String>) -> Result<(), String> {
    let data = data_home()?;
    let f = linux_files(&data);
    for path in [&f.desktop, &f.mime, &f.png, &f.svg] {
        remove_file(path, done)?;
    }
    if !done.is_empty() {
        linux_refresh(&data, done);
    }
    Ok(())
}

fn linux_refresh(data: &Path, done: &mut Vec<String>) {
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    run_optional("update-mime-database", &[&s(data.join("mime"))], done);
    run_optional("update-desktop-database", &[&s(data.join("applications"))], done);
    if data.join("icons/hicolor/index.theme").exists() {
        run_optional("gtk-update-icon-cache", &["-f", "-t", &s(data.join("icons/hicolor"))], done);
    }
}

/// The `.desktop` file (freedesktop Desktop Entry Specification).
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=hardcalc\n\
         GenericName=Spreadsheet\n\
         Comment=Terminal spreadsheet stored as readable Markdown\n\
         Exec={} %f\n\
         TryExec={}\n\
         Icon=hardcalc\n\
         Terminal=true\n\
         Categories=Office;Spreadsheet;ConsoleOnly;\n\
         MimeType={MIME_TYPE};\n\
         Keywords=spreadsheet;calculation;markdown;units;\n",
        desktop_exec_arg(&exe.to_string_lossy()),
        desktop_string(&exe.to_string_lossy()),
    )
}

/// One argument of `Exec=`: quoted when needed, then escaped as a string value.
fn desktop_exec_arg(arg: &str) -> String {
    let plain = !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "/._+-,:@".contains(c));
    let quoted = if plain {
        arg.to_string()
    } else {
        let mut q = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    };
    desktop_string(&quoted).replace('%', "%%")
}

/// Escapes a string value of a desktop entry.
fn desktop_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
}

/// The shared-mime-info package for `*.hcmd`.
pub fn mime_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
         \x20 <mime-type type=\"{MIME_TYPE}\">\n\
         \x20   <comment>hardcalc document</comment>\n\
         \x20   <sub-class-of type=\"text/markdown\"/>\n\
         \x20   <icon name=\"hardcalc\"/>\n\
         \x20   <glob pattern=\"*.hcmd\"/>\n\
         \x20 </mime-type>\n\
         </mime-info>\n"
    )
}

// ---------------------------------------------------------------- macOS

const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

fn macos_app() -> Result<PathBuf, String> {
    Ok(home()?.join("Applications/hardcalc.app"))
}

/// Whether `app` is a launcher written by `desktop install` (so it may be replaced).
fn is_our_app(app: &Path) -> bool {
    fs::read_to_string(app.join("Contents/Info.plist")).is_ok_and(|p| p.contains(BUNDLE_ID))
}

fn macos_install(exe: &Path, done: &mut Vec<String>) -> Result<(), String> {
    let app = macos_app()?;
    let app_s = app.to_string_lossy().into_owned();
    if app.exists() {
        if !is_our_app(&app) {
            return Err(format!("{} exists and was not made by hardcalc – remove it first", app.display()));
        }
        fs::remove_dir_all(&app).map_err(|e| format!("{}: {e}", app.display()))?;
    }
    if let Some(dir) = app.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    let script = env::temp_dir().join(format!("hardcalc-launcher-{}.applescript", std::process::id()));
    fs::write(&script, applescript(exe)).map_err(|e| format!("{}: {e}", script.display()))?;
    let compiled = run("osacompile", &["-o", &app_s, &script.to_string_lossy()]);
    let _ = fs::remove_file(&script);
    compiled?;

    let resources = app.join("Contents/Resources");
    fs::write(resources.join("applet.icns"), ICON_ICNS).map_err(|e| format!("{}: {e}", resources.display()))?;
    // An asset catalog would override applet.icns; droplet.icns is the
    // default icon osacompile adds for scripts with an `open` handler.
    let _ = fs::remove_file(resources.join("Assets.car"));
    let _ = fs::remove_file(resources.join("droplet.icns"));

    let plist = app.join("Contents/Info.plist");
    let plist_s = plist.to_string_lossy().into_owned();
    let _ = run("plutil", &["-remove", "CFBundleIconName", &plist_s]);
    let version = env!("CARGO_PKG_VERSION");
    for (key, kind, value) in [
        ("CFBundleIdentifier", "-string", BUNDLE_ID.to_string()),
        ("CFBundleName", "-string", "hardcalc".to_string()),
        ("CFBundleShortVersionString", "-string", version.to_string()),
        ("CFBundleIconFile", "-string", "applet".to_string()),
        ("CFBundleDocumentTypes", "-json", document_types_json()),
        ("UTExportedTypeDeclarations", "-json", exported_types_json()),
    ] {
        run("plutil", &["-replace", key, kind, &value, &plist_s])?;
    }
    // Changing Info.plist invalidates the signature osacompile made.
    run("codesign", &["--force", "--sign", "-", &app_s])?;
    done.push(format!("wrote {} (opens hardcalc in Terminal)", app.display()));

    run(LSREGISTER, &["-f", &app_s])?;
    done.push(format!("registered .hcmd ({UTI}) with LaunchServices"));
    match set_default_handler() {
        Ok(()) => done.push("hardcalc.app is now the default app for .hcmd".into()),
        Err(e) => done.push(format!("note: could not make hardcalc.app the default for .hcmd ({e}); \
                                     use Get Info → Open with → Change All in Finder")),
    }
    Ok(())
}

/// Makes the launcher the default for `.hcmd` (LaunchServices, via JXA).
fn set_default_handler() -> Result<(), String> {
    let js = format!(
        "ObjC.import('CoreServices'); \
         $.LSSetDefaultRoleHandlerForContentType($('{UTI}'), 0xFFFFFFFF, $('{BUNDLE_ID}'))"
    );
    let status = run("osascript", &["-l", "JavaScript", "-e", &js])?;
    match status.trim() {
        "0" => Ok(()),
        other => Err(format!("status {other}")),
    }
}

fn macos_uninstall(done: &mut Vec<String>) -> Result<(), String> {
    let app = macos_app()?;
    if !app.exists() {
        return Ok(());
    }
    if !is_our_app(&app) {
        return Err(format!("{} was not made by hardcalc – left alone", app.display()));
    }
    let _ = run(LSREGISTER, &["-u", &app.to_string_lossy()]);
    fs::remove_dir_all(&app).map_err(|e| format!("{}: {e}", app.display()))?;
    done.push(format!("removed {}", app.display()));
    Ok(())
}

/// The launcher: runs hardcalc in a new Terminal window, once per opened file.
pub fn applescript(exe: &Path) -> String {
    let command = format!("clear; exec {}", sh_quote(&exe.to_string_lossy()));
    format!(
        r#"on run
	launch_hardcalc("")
end run

on open theFiles
	repeat with f in theFiles
		launch_hardcalc(" " & quoted form of POSIX path of f)
	end repeat
end open

on launch_hardcalc(arg)
	set cmd to "{}" & arg
	set wasRunning to application "Terminal" is running
	tell application "Terminal"
		activate
		if wasRunning then
			do script cmd
		else
			-- Terminal opens a window when it starts; use it instead of a second one.
			repeat 50 times
				if (count of windows) > 0 then exit repeat
				delay 0.1
			end repeat
			if (count of windows) > 0 then
				do script cmd in window 1
			else
				do script cmd
			end if
		end if
	end tell
end launch_hardcalc
"#,
        applescript_string(&command)
    )
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn document_types_json() -> String {
    format!(
        r#"[{{"CFBundleTypeName":"hardcalc document","CFBundleTypeRole":"Editor","LSHandlerRank":"Owner","LSItemContentTypes":["{UTI}"],"CFBundleTypeIconFile":"applet"}}]"#
    )
}

fn exported_types_json() -> String {
    format!(
        r#"[{{"UTTypeIdentifier":"{UTI}","UTTypeDescription":"hardcalc document","UTTypeConformsTo":["public.plain-text"],"UTTypeIconFile":"applet","UTTypeReferenceURL":"https://hardcalc.org","UTTypeTagSpecification":{{"public.filename-extension":["hcmd"]}}}}]"#
    )
}

// ---------------------------------------------------------------- Windows

fn windows_run(action: &str, exe: &Path, done: &mut Vec<String>) -> Result<(), String> {
    let base = env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("LOCALAPPDATA is not set")?;
    let ico = base.join("hardcalc").join("hardcalc.ico");
    if action == "install" {
        write_file(&ico, ICON_ICO, done)?;
    }
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(powershell_encode(WINDOWS_SCRIPT))
        .env("HARDCALC_ACTION", action)
        .env("HARDCALC_EXE", exe)
        .env("HARDCALC_ICO", &ico)
        .output()
        .map_err(|e| format!("powershell.exe: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    done.extend(stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string));
    if !out.status.success() {
        return Err(format!("powershell.exe: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    if action == "uninstall" {
        remove_file(&ico, done)?;
        let _ = fs::remove_dir(base.join("hardcalc"));
    }
    Ok(())
}

/// Registry entries, start menu shortcut and shell refresh. Paths come in
/// through environment variables, so nothing needs quoting.
const WINDOWS_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$classes = 'HKCU:\Software\Classes'
$progId = 'hardcalc.hcmd'
$ext = "$classes\.hcmd"
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'hardcalc.lnk'

function Ensure-Key($path) {
    if (-not (Test-Path -LiteralPath $path)) { New-Item -Path $path -Force | Out-Null }
}

if ($env:HARDCALC_ACTION -eq 'install') {
    $exe = $env:HARDCALC_EXE
    $ico = $env:HARDCALC_ICO
    Ensure-Key "$classes\$progId"
    Set-Item -LiteralPath "$classes\$progId" -Value 'hardcalc document'
    Ensure-Key "$classes\$progId\DefaultIcon"
    Set-Item -LiteralPath "$classes\$progId\DefaultIcon" -Value "`"$ico`",0"
    Ensure-Key "$classes\$progId\shell\open\command"
    Set-Item -LiteralPath "$classes\$progId\shell\open\command" -Value "`"$exe`" `"%1`""
    Ensure-Key $ext
    Set-Item -LiteralPath $ext -Value $progId
    Set-ItemProperty -LiteralPath $ext -Name 'PerceivedType' -Value 'text'
    Ensure-Key "$ext\OpenWithProgids"
    Set-ItemProperty -LiteralPath "$ext\OpenWithProgids" -Name $progId -Value ''
    "registered .hcmd ($progId) under HKCU\Software\Classes"

    $shell = New-Object -ComObject WScript.Shell
    $s = $shell.CreateShortcut($lnk)
    $s.TargetPath = $exe
    $s.IconLocation = "$ico,0"
    $s.Description = 'Terminal spreadsheet stored as readable Markdown'
    $s.WorkingDirectory = [Environment]::GetFolderPath('MyDocuments')
    $s.Save()
    "wrote $lnk"
} else {
    if (Test-Path -LiteralPath "$classes\$progId") {
        Remove-Item -LiteralPath "$classes\$progId" -Recurse -Force
        "removed HKCU\Software\Classes\$progId"
    }
    if (Test-Path -LiteralPath $ext) {
        if ((Get-Item -LiteralPath $ext).GetValue('') -eq $progId) { Set-Item -LiteralPath $ext -Value '' }
        if (Test-Path -LiteralPath "$ext\OpenWithProgids") {
            Remove-ItemProperty -LiteralPath "$ext\OpenWithProgids" -Name $progId -ErrorAction SilentlyContinue
        }
    }
    if (Test-Path -LiteralPath $lnk) {
        Remove-Item -LiteralPath $lnk -Force
        "removed $lnk"
    }
}

# Tell Explorer that file associations changed (SHCNE_ASSOCCHANGED).
Add-Type -Namespace HardCalc -Name Shell -MemberDefinition '[System.Runtime.InteropServices.DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, System.IntPtr a, System.IntPtr b);'
[HardCalc.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"#;

/// Base64 of the UTF-16LE text, as `powershell -EncodedCommand` expects it.
fn powershell_encode(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        for (input, expected) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn exec_quotes_only_when_needed() {
        assert_eq!(desktop_exec_arg("/home/a/.cargo/bin/hardcalc"), "/home/a/.cargo/bin/hardcalc");
        assert_eq!(desktop_exec_arg("/opt/my apps/hardcalc"), "\"/opt/my apps/hardcalc\"");
        // `$` is escaped for the quoting, the backslash again for the string value.
        assert_eq!(desktop_exec_arg("/x/a$b"), "\"/x/a\\\\$b\"");
        assert_eq!(desktop_exec_arg("/x/100%"), "\"/x/100%%\"");
    }

    #[test]
    fn desktop_entry_opens_files_in_a_terminal() {
        let entry = desktop_entry(Path::new("/usr/bin/hardcalc"));
        assert!(entry.contains("Exec=/usr/bin/hardcalc %f\n"));
        assert!(entry.contains("Terminal=true\n"));
        assert!(entry.contains("MimeType=text/x-hardcalc;\n"));
        assert!(mime_xml().contains("<glob pattern=\"*.hcmd\"/>"));
    }

    #[test]
    fn applescript_quotes_the_path_for_shell_and_applescript() {
        let script = applescript(Path::new("/Users/o'neil/bin/hard\"calc"));
        assert!(script.contains(r#"set cmd to "clear; exec '/Users/o'\\''neil/bin/hard\"calc'" & arg"#), "{script}");
    }

    #[test]
    fn plist_fragments_are_json() {
        for json in [document_types_json(), exported_types_json()] {
            serde_json::from_str::<serde_json::Value>(&json).unwrap();
        }
    }
}
