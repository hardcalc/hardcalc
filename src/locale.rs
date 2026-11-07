//! How numbers are shown: with a decimal point or a decimal comma.
//!
//! Files always hold numbers with a decimal point (`1200.50`). Only the
//! display follows the user: the programs call [`init_from_env`] at start,
//! which reads `HARDCALC_DECIMAL=comma|point` or else the locale (LC_ALL,
//! LC_NUMERIC, LANG). Without it – in tests and in the library – numbers are
//! shown with a point.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

static COMMA: AtomicBool = AtomicBool::new(false);

thread_local! {
    static THIS_THREAD: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether numbers are shown with a decimal comma.
pub fn decimal_comma() -> bool {
    THIS_THREAD.with(Cell::get).unwrap_or_else(|| COMMA.load(Ordering::Relaxed))
}

/// Sets the display from the environment; called once by the programs.
pub fn init_from_env() {
    COMMA.store(env_uses_comma(), Ordering::Relaxed);
}

/// Overrides the display for the current thread only (tests).
pub fn set_decimal_comma_for_this_thread(comma: bool) {
    THIS_THREAD.with(|c| c.set(Some(comma)));
}

/// `HARDCALC_DECIMAL=comma|point`, otherwise whether the locale writes
/// decimals with a comma.
pub fn env_uses_comma() -> bool {
    match std::env::var("HARDCALC_DECIMAL").ok().as_deref().map(str::trim) {
        Some("comma") | Some(",") => return true,
        Some("point") | Some(".") => return false,
        _ => {}
    }
    let lang = ["LC_ALL", "LC_NUMERIC", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    let lang = lang.unwrap_or_default().to_lowercase();
    ["de", "fr", "es", "it", "nl", "pt", "ru", "pl", "cs", "sk", "sv", "da", "fi", "nb", "nn", "tr", "hu", "el", "ro"]
        .iter()
        .any(|l| lang.starts_with(l))
}

/// A number written with a point, as shown: `1200.50` → `1200,50` with the
/// decimal comma.
pub fn show(text: &str) -> String {
    if decimal_comma() { text.replace('.', ",") } else { text.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_thread_override() {
        assert!(!decimal_comma());
        set_decimal_comma_for_this_thread(true);
        assert!(decimal_comma());
        assert_eq!(show("1.5"), "1,5");
        let other = std::thread::spawn(decimal_comma).join().unwrap();
        assert!(!other, "other threads keep the default");
    }
}
