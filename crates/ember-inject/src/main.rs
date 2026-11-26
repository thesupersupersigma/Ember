//! ember.exe — the launcher.
//!
//! Plan (decision 1.6): start Discord.exe suspended, write the path to ember.dll
//! into it, queue LoadLibraryW as an APC on its main thread, then resume — so our
//! DLL loads before Discord's own code and past its System32-only import
//! hardening.

fn main() {
    #[cfg(not(windows))]
    eprintln!("ember-inject only runs on Windows");
    // TODO(v1): injection.
}
