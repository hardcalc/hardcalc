//! The one source of all key and mouse bindings.
//!
//! [`KEYMAP`] lists every way to reach a program function ([`Action`]):
//! keys and mouse. From it come the dispatch of
//! keys in the table views, the keys shown in the context menu and the
//! help, and the documentation matrix `docs/keys.md`
//! (`hardcalc --keymap-markdown`). Inputs that only work in some
//! terminals carry a [`Cond`]; [`Caps`] says which of them work here, and
//! only those that are guaranteed or confirmed are shown.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A program function reachable by key, mouse or menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ContextMenu,
    Menu,
    Help,
    Save,
    Quit,
    CommandLine,
    OpenFile,
    Goto,
    EditCell,
    ClearCells,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    FillDown,
    FillRight,
    InsertRow,
    InsertColumn,
    DeleteLines,
    Sort,
    Pivot,
    Split,
    SwapWindows,
    NextTable,
    PrevTable,
    Bold,
}

impl Action {
    pub const ALL: [Action; 27] = [
        Action::ContextMenu,
        Action::Menu,
        Action::Help,
        Action::Save,
        Action::Quit,
        Action::CommandLine,
        Action::OpenFile,
        Action::Goto,
        Action::EditCell,
        Action::ClearCells,
        Action::Copy,
        Action::Cut,
        Action::Paste,
        Action::Undo,
        Action::Redo,
        Action::FillDown,
        Action::FillRight,
        Action::InsertRow,
        Action::InsertColumn,
        Action::DeleteLines,
        Action::Sort,
        Action::Pivot,
        Action::Split,
        Action::SwapWindows,
        Action::NextTable,
        Action::PrevTable,
        Action::Bold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::ContextMenu => "Context menu",
            Action::Menu => "Menu bar",
            Action::Help => "Help",
            Action::Save => "Save",
            Action::Quit => "Quit",
            Action::CommandLine => "Command line",
            Action::OpenFile => "Open file",
            Action::Goto => "Go to cell / follow link",
            Action::EditCell => "Edit cell",
            Action::ClearCells => "Clear cells",
            Action::Copy => "Copy",
            Action::Cut => "Cut",
            Action::Paste => "Paste",
            Action::Undo => "Undo / jump back",
            Action::Redo => "Redo",
            Action::FillDown => "Fill down",
            Action::FillRight => "Fill right",
            Action::InsertRow => "Insert row",
            Action::InsertColumn => "Insert column",
            Action::DeleteLines => "Delete rows / columns",
            Action::Sort => "Sort by column",
            Action::Pivot => "Pivot view",
            Action::Split => "Split window",
            Action::SwapWindows => "Swap windows",
            Action::NextTable => "Next table",
            Action::PrevTable => "Previous table",
            Action::Bold => "Bold on / off",
        }
    }
}

/// Kind of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key,
    Mouse,
}

/// What a binding needs to work in every terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cond {
    /// The Menu key reaches only terminals with the kitty keyboard protocol.
    MenuKey,
    /// Shift+F10: not usual on macOS, GNOME Terminal (VTE) takes it.
    ShiftF10,
    /// The terminal reports mouse clicks.
    Mouse,
}

impl Cond {
    pub const ALL: [Cond; 3] = [Cond::MenuKey, Cond::ShiftF10, Cond::Mouse];

    /// Footnote of the documentation matrix.
    pub fn note(self) -> &'static str {
        match self {
            Cond::MenuKey => "only with the kitty keyboard protocol (kitty, WezTerm, foot, Ghostty); not on macOS",
            Cond::ShiftF10 => "not on macOS; GNOME Terminal (VTE) intercepts it",
            Cond::Mouse => "if the terminal reports the mouse (almost all do)",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Cond::MenuKey => "menu_key",
            Cond::ShiftF10 => "shift_f10",
            Cond::Mouse => "mouse",
        }
    }
}

/// The key of a binding, for dispatch (mouse and menu sequences are only described).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Key(KeyCode, KeyModifiers),
    /// Described only: a click, a gesture, a menu sequence.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub action: Action,
    pub input: Input,
    /// As shown: `Shift+F10`, `right-click`, `/ R I`.
    pub label: &'static str,
    pub trigger: Trigger,
    pub cond: Option<Cond>,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

const fn key(action: Action, label: &'static str, code: KeyCode, mods: KeyModifiers) -> Binding {
    Binding { action, input: Input::Key, label, trigger: Trigger::Key(code, mods), cond: None }
}

const fn key_if(action: Action, label: &'static str, code: KeyCode, mods: KeyModifiers, cond: Cond) -> Binding {
    Binding { action, input: Input::Key, label, trigger: Trigger::Key(code, mods), cond: Some(cond) }
}

/// A key sequence through the context menu: shown, not dispatched here.
const fn seq(action: Action, label: &'static str) -> Binding {
    Binding { action, input: Input::Key, label, trigger: Trigger::Other, cond: None }
}

const fn mouse(action: Action, label: &'static str) -> Binding {
    Binding { action, input: Input::Mouse, label, trigger: Trigger::Other, cond: Some(Cond::Mouse) }
}

/// All bindings. The order is the order in help and documentation.
pub static KEYMAP: &[Binding] = &[
    key(Action::ContextMenu, "/", KeyCode::Char('/'), NONE),
    key_if(Action::ContextMenu, "Menu key", KeyCode::Menu, NONE, Cond::MenuKey),
    key_if(Action::ContextMenu, "Shift+F10", KeyCode::F(10), SHIFT, Cond::ShiftF10),
    mouse(Action::ContextMenu, "right-click"),
    mouse(Action::ContextMenu, "Ctrl+click"),
    key(Action::Menu, "F9", KeyCode::F(9), NONE),
    mouse(Action::Menu, "click ≡ hardcalc"),
    key(Action::Help, "F1", KeyCode::F(1), NONE),
    mouse(Action::Help, "click F1"),
    key(Action::Save, "F2", KeyCode::F(2), NONE),
    key(Action::Save, "Ctrl+S", KeyCode::Char('s'), CTRL),
    mouse(Action::Save, "click F2"),
    key(Action::Quit, "F10", KeyCode::F(10), NONE),
    key(Action::Quit, "Ctrl+Q", KeyCode::Char('q'), CTRL),
    mouse(Action::Quit, "click F10"),
    key(Action::CommandLine, ":", KeyCode::Char(':'), NONE),
    key(Action::OpenFile, "F3", KeyCode::F(3), NONE),
    key(Action::OpenFile, "Ctrl+E", KeyCode::Char('e'), CTRL),
    seq(Action::OpenFile, "F9 File Open"),
    mouse(Action::OpenFile, "click F3"),
    key(Action::Goto, "F4", KeyCode::F(4), NONE),
    mouse(Action::Goto, "click F4"),
    mouse(Action::Goto, "click ↗"),
    key(Action::EditCell, "Enter", KeyCode::Enter, NONE),
    mouse(Action::EditCell, "double-click"),
    key(Action::ClearCells, "Del", KeyCode::Delete, NONE),
    key(Action::ClearCells, "Backspace", KeyCode::Backspace, NONE),
    seq(Action::ClearCells, "/ E L"),
    key(Action::Copy, "Ctrl+C", KeyCode::Char('c'), CTRL),
    seq(Action::Copy, "/ E C"),
    key(Action::Cut, "Ctrl+X", KeyCode::Char('x'), CTRL),
    seq(Action::Cut, "/ E X"),
    key(Action::Paste, "Ctrl+V", KeyCode::Char('v'), CTRL),
    seq(Action::Paste, "/ E P"),
    key(Action::Undo, "Ctrl+Z", KeyCode::Char('z'), CTRL),
    seq(Action::Undo, "/ E U"),
    seq(Action::Undo, ":back"),
    mouse(Action::Undo, "click ← in the status line"),
    key(Action::Redo, "Ctrl+Y", KeyCode::Char('y'), CTRL),
    seq(Action::Redo, "/ E R"),
    key(Action::FillDown, "Ctrl+D", KeyCode::Char('d'), CTRL),
    seq(Action::FillDown, "/ R F"),
    mouse(Action::FillDown, "drag ▪"),
    key(Action::FillRight, "Ctrl+R", KeyCode::Char('r'), CTRL),
    seq(Action::FillRight, "/ C F"),
    key(Action::InsertRow, "F5", KeyCode::F(5), NONE),
    key(Action::InsertRow, "Ins", KeyCode::Insert, NONE),
    seq(Action::InsertRow, "/ R I"),
    mouse(Action::InsertRow, "click F5"),
    mouse(Action::InsertRow, "context menu"),
    key(Action::InsertColumn, "F6", KeyCode::F(6), NONE),
    seq(Action::InsertColumn, "/ C I"),
    mouse(Action::InsertColumn, "click F6"),
    key(Action::DeleteLines, "F8", KeyCode::F(8), NONE),
    seq(Action::DeleteLines, "/ R D"),
    mouse(Action::DeleteLines, "click F8"),
    mouse(Action::DeleteLines, "context menu"),
    key(Action::Sort, "F7", KeyCode::F(7), NONE),
    seq(Action::Sort, "/ C S"),
    mouse(Action::Sort, "click F7"),
    key(Action::Pivot, "Ctrl+P", KeyCode::Char('p'), CTRL),
    seq(Action::Pivot, "/ T P"),
    mouse(Action::Pivot, "click ⊞ pivot"),
    key(Action::Split, "Ctrl+O", KeyCode::Char('o'), CTRL),
    seq(Action::Split, "/ V S"),
    key(Action::SwapWindows, "Ctrl+U", KeyCode::Char('u'), CTRL),
    seq(Action::SwapWindows, "/ V W"),
    key(Action::NextTable, "Ctrl+PgDn", KeyCode::PageDown, CTRL),
    mouse(Action::NextTable, "click a tab"),
    key(Action::PrevTable, "Ctrl+PgUp", KeyCode::PageUp, CTRL),
    mouse(Action::PrevTable, "click a tab"),
    key(Action::Bold, "Ctrl+B", KeyCode::Char('b'), CTRL),
    seq(Action::Bold, "/ F B"),
];

/// How sure it is that an input works in this terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Always works.
    Guaranteed,
    /// Seen working (received, or the key test).
    Confirmed,
    Unknown,
    /// Cannot work here (platform, terminal, or the key test).
    Impossible,
}

impl Cap {
    pub fn shown(self) -> bool {
        matches!(self, Cap::Guaranteed | Cap::Confirmed)
    }

    fn name(self) -> &'static str {
        match self {
            Cap::Guaranteed => "guaranteed",
            Cap::Confirmed => "confirmed",
            Cap::Unknown => "unknown",
            Cap::Impossible => "impossible",
        }
    }

    fn parse(s: &str) -> Option<Cap> {
        match s {
            "guaranteed" => Some(Cap::Guaranteed),
            "confirmed" => Some(Cap::Confirmed),
            "unknown" => Some(Cap::Unknown),
            "impossible" => Some(Cap::Impossible),
            _ => None,
        }
    }
}

/// What the conditional inputs can do in this terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub menu_key: Cap,
    pub shift_f10: Cap,
    pub mouse: Cap,
}

impl Default for Caps {
    /// Before anything is known: only what always works.
    fn default() -> Caps {
        Caps { menu_key: Cap::Unknown, shift_f10: Cap::Unknown, mouse: Cap::Unknown }
    }
}

impl Caps {
    pub fn get(&self, c: Cond) -> Cap {
        match c {
            Cond::MenuKey => self.menu_key,
            Cond::ShiftF10 => self.shift_f10,
            Cond::Mouse => self.mouse,
        }
    }

    pub fn set(&mut self, c: Cond, cap: Cap) {
        let slot = match c {
            Cond::MenuKey => &mut self.menu_key,
            Cond::ShiftF10 => &mut self.shift_f10,
            Cond::Mouse => &mut self.mouse,
        };
        // What the platform rules out stays ruled out.
        if *slot != Cap::Impossible || cap == Cap::Impossible {
            *slot = cap;
        }
    }

    /// From platform and terminal: `env` reads environment variables,
    /// `kitty` says whether the terminal speaks the kitty keyboard protocol.
    pub fn detect(env: impl Fn(&str) -> Option<String>, macos: bool, kitty: bool) -> Caps {
        let mut caps = Caps::default();
        if macos {
            caps.menu_key = Cap::Impossible;
            caps.shift_f10 = Cap::Impossible;
        }
        if !kitty {
            caps.menu_key = Cap::Impossible;
        }
        // GNOME Terminal and other VTE terminals open their own menu on Shift+F10.
        if env("VTE_VERSION").is_some() {
            caps.shift_f10 = Cap::Impossible;
        }
        caps
    }

    /// Whether a binding works here as far as known.
    pub fn allows(&self, b: &Binding) -> bool {
        b.cond.is_none_or(|c| self.get(c).shown())
    }

    /// Notes an input that arrived: it works.
    pub fn observe(&mut self, key: &KeyEvent) -> bool {
        let cond = match (key.code, key.modifiers) {
            (KeyCode::Menu, _) => Cond::MenuKey,
            (KeyCode::F(10), m) if m == SHIFT => Cond::ShiftF10,
            _ => return false,
        };
        let before = self.get(cond);
        self.set(cond, Cap::Confirmed);
        before != self.get(cond)
    }

    /// `menu_key = confirmed` lines for the config file.
    pub fn to_lines(&self) -> String {
        Cond::ALL.iter().map(|c| format!("{} = \"{}\"\n", c.name(), self.get(*c).name())).collect()
    }

    /// Applies confirmed / impossible states remembered from earlier runs.
    pub fn merge_lines(&mut self, text: &str) {
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Some(cap) = Cap::parse(v.trim().trim_matches('"')) else { continue };
            if let Some(c) = Cond::ALL.iter().find(|c| c.name() == k.trim())
                && matches!(cap, Cap::Confirmed | Cap::Impossible)
            {
                self.set(*c, cap);
            }
        }
    }
}

/// Where observed capabilities are remembered: one file per terminal and platform.
pub fn caps_file(env: impl Fn(&str) -> Option<String>) -> Option<std::path::PathBuf> {
    let base = env("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| env("APPDATA").map(std::path::PathBuf::from))
        .or_else(|| env("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    let term = env("TERM_PROGRAM").or_else(|| env("TERM")).unwrap_or_else(|| "terminal".into());
    let term: String = term.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    Some(base.join("hardcalc").join(format!("caps-{term}-{}.toml", std::env::consts::OS)))
}

/// The action bound to a key in the table views, if any.
pub fn action_for(key: &KeyEvent, caps: &Caps) -> Option<Action> {
    // Shift is part of a character ('/' is Shift+7 on German keyboards).
    let mods = match key.code {
        KeyCode::Char(_) => key.modifiers - KeyModifiers::SHIFT,
        _ => key.modifiers,
    };
    KEYMAP
        .iter()
        .filter(|b| caps.allows(b))
        .find(|b| b.trigger == Trigger::Key(key.code, mods))
        .map(|b| b.action)
}

/// The keys of an action shown to the user (`F2 · Ctrl+S`).
pub fn keys_for(action: Action, caps: &Caps) -> String {
    KEYMAP
        .iter()
        .filter(|b| b.action == action && b.input == Input::Key && matches!(b.trigger, Trigger::Key(..)) && caps.allows(b))
        .map(|b| b.label)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Help lines (keys, function) for this terminal: only inputs that work.
pub fn help_lines(caps: &Caps) -> Vec<(String, &'static str)> {
    Action::ALL
        .iter()
        .map(|a| {
            let inputs: Vec<&str> = KEYMAP.iter().filter(|b| b.action == *a && caps.allows(b)).map(|b| b.label).collect();
            (inputs.join(" · "), a.label())
        })
        .collect()
}

/// The documentation matrix: every function × input, with
/// footnotes for conditional inputs. `–`: not with this input.
pub fn markdown() -> String {
    let mut notes: Vec<Cond> = Vec::new();
    let mut cell = |action: Action, input: Input| -> String {
        let parts: Vec<String> = KEYMAP
            .iter()
            .filter(|b| b.action == action && b.input == input)
            .map(|b| {
                let label = format!("`{}`", b.label);
                match b.cond.filter(|c| *c != Cond::Mouse) {
                    Some(c) => {
                        if !notes.contains(&c) {
                            notes.push(c);
                        }
                        let n = notes.iter().position(|x| *x == c).unwrap() + 1;
                        format!("{label}{}", superscript(n))
                    }
                    None => label,
                }
            })
            .collect();
        if parts.is_empty() { "–".to_string() } else { parts.join(" · ") }
    };
    let mut rows = Vec::new();
    for a in Action::ALL {
        rows.push(format!(
            "| {} | {} | {} |",
            a.label(),
            cell(a, Input::Key),
            cell(a, Input::Mouse),
        ));
    }
    let mut out = String::new();
    out.push_str("# hardcalc – keys and mouse\n\n");
    out.push_str("<!-- Generated by `hardcalc --keymap-markdown` from src/tui/keymap.rs – do not edit by hand. -->\n\n");
    out.push_str("Every function with all ways to trigger it, per input type.\n");
    out.push_str("`–` means: not available with this input type. Sequences like `/ R I` go through the\n");
    out.push_str("context menu (`/`, then the marked letters).\n\n");
    out.push_str("| Function | Keyboard | Mouse |\n");
    out.push_str("|---|---|---|\n");
    for r in rows {
        out.push_str(&r);
        out.push('\n');
    }
    if !notes.is_empty() {
        out.push('\n');
        let parts: Vec<String> = notes.iter().enumerate().map(|(i, c)| format!("{} {}", superscript(i + 1), c.note())).collect();
        out.push_str(&parts.join(" · "));
        out.push('\n');
    }
    out
}

fn superscript(n: usize) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            _ => '⁰',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_guaranteed_key_in_the_terminal() {
        for a in Action::ALL {
            let ok = KEYMAP.iter().any(|b| b.action == a && b.input == Input::Key && b.cond.is_none() && matches!(b.trigger, Trigger::Key(..)));
            assert!(ok, "{a:?} has no guaranteed key in the TUI");
        }
    }

    #[test]
    fn no_key_is_bound_twice() {
        for (i, a) in KEYMAP.iter().enumerate() {
            for b in &KEYMAP[i + 1..] {
                if let Trigger::Key(..) = a.trigger {
                    assert!(a.trigger != b.trigger, "{} bound to {:?} and {:?}", a.label, a.action, b.action);
                }
            }
        }
    }

    #[test]
    fn dispatch_respects_capabilities() {
        let shift_f10 = KeyEvent::new(KeyCode::F(10), SHIFT);
        let mut caps = Caps::detect(|_| None, false, false);
        assert_eq!(action_for(&shift_f10, &caps), None);
        assert!(caps.observe(&shift_f10));
        assert_eq!(action_for(&shift_f10, &caps), Some(Action::ContextMenu));
        // '/' with Shift (German keyboard) is still '/'.
        assert_eq!(action_for(&KeyEvent::new(KeyCode::Char('/'), SHIFT), &caps), Some(Action::ContextMenu));
        assert_eq!(action_for(&KeyEvent::new(KeyCode::F(10), NONE), &caps), Some(Action::Quit));
    }

    #[test]
    fn platform_and_terminal_rule_out_inputs() {
        let mac = Caps::detect(|_| None, true, true);
        assert_eq!((mac.menu_key, mac.shift_f10), (Cap::Impossible, Cap::Impossible));
        let vte = Caps::detect(|k| (k == "VTE_VERSION").then(|| "7600".into()), false, true);
        assert_eq!((vte.menu_key, vte.shift_f10), (Cap::Unknown, Cap::Impossible));
        // A remembered confirmation does not override the platform.
        let mut mac = mac;
        mac.merge_lines("shift_f10 = \"confirmed\"\n");
        assert_eq!(mac.shift_f10, Cap::Impossible);
        let mut linux = Caps::detect(|_| None, false, true);
        linux.merge_lines(&Caps { menu_key: Cap::Confirmed, ..linux }.to_lines());
        assert_eq!(linux.menu_key, Cap::Confirmed);
    }

    #[test]
    fn help_shows_only_what_works() {
        let caps = Caps::detect(|_| None, true, false);
        let lines = help_lines(&caps);
        let ctx = &lines.iter().find(|(_, l)| *l == "Context menu").unwrap().0;
        assert!(ctx.starts_with('/') && !ctx.contains("Shift+F10") && !ctx.contains("Menu key"), "{ctx}");
        assert_eq!(keys_for(Action::Save, &caps), "F2 · Ctrl+S");
    }

    #[test]
    fn documentation_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/keys.md");
        let doc = std::fs::read_to_string(path).unwrap_or_default();
        assert!(doc == markdown(), "docs/keys.md is outdated – run: cargo run -- --keymap-markdown > docs/keys.md");
    }
}
