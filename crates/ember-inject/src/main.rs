//! `ember-launcher.exe` -- the Ember launcher and installer.
//!
//! Run-mode detection rule (documented here as the spec requires):
//!
//!   If the running exe's path is under `%LOCALAPPDATA%\Ember\`
//!     -> inject+launch mode (start Discord with ember.dll).
//!   `--install`  -> headless install (no GUI), same file copy + shortcuts.
//!   `--uninstall` -> remove shortcuts and the Ember directory.
//!   Otherwise (not installed, no CLI flags) -> show the installer wizard.
//!
//! This is Windows-only. On other targets it prints a notice and exits.

// Hide the console window: the wizard is GUI, inject mode launches from a
// shortcut, and --install/--uninstall report results via message boxes.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(windows, allow(clippy::missing_safety_doc))]

#[cfg(not(windows))]
fn main() {
    eprintln!("ember-inject only runs on Windows.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(e) = win::run() {
        win::alert(&format!("{e}"));
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod win;
