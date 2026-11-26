pub mod install;
pub mod wizard;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{env, ptr};

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, QueueUserAPC, ResumeThread, TerminateProcess, CREATE_SUSPENDED,
    PROCESS_INFORMATION, STARTUPINFOW,
};

pub type Result<T> = std::result::Result<T, String>;

// ── Run-mode detection ──────────────────────────────────────────────────

/// True when the running exe is under `%LOCALAPPDATA%\Ember\`.
fn is_installed_location() -> bool {
    is_under_ember_dir(
        env::current_exe().ok().as_deref(),
        env::var_os("LOCALAPPDATA").map(PathBuf::from).as_deref(),
    )
}

/// Pure-logic helper so the detection rule is unit-testable.
fn is_under_ember_dir(exe: Option<&Path>, local_appdata: Option<&Path>) -> bool {
    match (exe, local_appdata) {
        (Some(exe), Some(local)) => exe.starts_with(local.join("Ember")),
        _ => false,
    }
}

#[derive(Debug, PartialEq)]
pub enum RunMode {
    Inject,
    Wizard,
    HeadlessInstall,
    Uninstall,
}

/// Decide what mode to run in from the CLI args and the exe's location.
pub fn determine_mode(args: &[String], exe_under_ember_dir: bool) -> RunMode {
    if args.first().map(|s| s.as_str()) == Some("--install") {
        return RunMode::HeadlessInstall;
    }
    if args.first().map(|s| s.as_str()) == Some("--uninstall") {
        return RunMode::Uninstall;
    }
    // Installed location -> inject. Any other flags (--dll, --exe, --help)
    // also mean inject mode (power user running from a build directory).
    if exe_under_ember_dir || !args.is_empty() {
        return RunMode::Inject;
    }
    RunMode::Wizard
}

// ── Entry point ─────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match determine_mode(&args, is_installed_location()) {
        RunMode::HeadlessInstall => install::headless_install(),
        RunMode::Uninstall => install::uninstall(),
        RunMode::Inject => inject_main(),
        RunMode::Wizard => wizard::show(),
    }
}

// ── Message boxes (no console with windows_subsystem = "windows") ───────

pub fn alert(text: &str) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Ember").as_ptr(),
            0x10, // MB_ICONERROR
        );
    }
}

pub fn info(text: &str) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Ember").as_ptr(),
            0x40, // MB_ICONINFORMATION
        );
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────

/// Encode a Rust string/path as a NUL-terminated UTF-16 buffer.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn default_discord_exe() -> PathBuf {
    let local = env::var_os("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local)
        .join("DiscordPhoenix")
        .join("app")
        .join("Discord.exe")
}

/// PowerShell.exe by its full System32 path so we never pick up something
/// planted beside the launcher.
pub fn powershell_path() -> PathBuf {
    PathBuf::from(env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

// ── Inject mode (existing launcher behaviour) ───────────────────────────

fn inject_main() -> Result<()> {
    let mut dll: Option<PathBuf> = None;
    let mut exe: Option<PathBuf> = None;
    let mut passthrough: Vec<String> = Vec::new();

    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dll" => dll = Some(args.next().ok_or("--dll needs a path")?.into()),
            "--exe" => exe = Some(args.next().ok_or("--exe needs a path")?.into()),
            "--" => {
                passthrough.extend(args.by_ref());
                break;
            }
            "-h" | "--help" => {
                info(
                    "Usage: ember-launcher [--dll <ember.dll>] \
                     [--exe <Discord.exe>] [-- <discord args>]",
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let exe = exe.unwrap_or_else(default_discord_exe);
    if !exe.is_file() {
        return Err(format!(
            "Discord.exe not found at {}\n(use --exe to point at it)",
            exe.display()
        ));
    }

    let dll = match dll {
        Some(p) => p,
        None => default_dll_beside_self()?,
    };
    let dll = dll
        .canonicalize()
        .map_err(|e| format!("resolving ember.dll path ({}): {e}", dll.display()))?;
    if !dll.is_file() {
        return Err(format!("ember.dll not found at {}", dll.display()));
    }

    inject_launch(&exe, &dll, &passthrough)
}

fn default_dll_beside_self() -> Result<PathBuf> {
    let me = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    Ok(me
        .parent()
        .ok_or("ember.exe has no parent dir")?
        .join("ember.dll"))
}

/// Start `exe` suspended, force `dll` to load first via an APC, then resume.
fn inject_launch(exe: &Path, dll: &Path, passthrough: &[String]) -> Result<()> {
    let mut cmdline = format!("\"{}\"", exe.display());
    for a in passthrough {
        cmdline.push(' ');
        cmdline.push_str(a);
    }
    let mut cmdline_w = wide(&cmdline);

    let app_dir = exe.parent().map(wide);

    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    // SAFETY: cmdline_w is a NUL-terminated writable UTF-16 buffer; pointers
    // are valid for the duration of the call.
    let ok = unsafe {
        CreateProcessW(
            wide(exe).as_ptr(),
            cmdline_w.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            FALSE,
            CREATE_SUSPENDED,
            ptr::null(),
            app_dir
                .as_ref()
                .map(|v| v.as_ptr())
                .unwrap_or(ptr::null()),
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(format!("CreateProcessW failed (os error {})", last_error()));
    }

    // From here on, any early return must terminate the suspended child so we
    // don't leave a frozen Discord process behind.
    let result = queue_loadlibrary(pi.hProcess, pi.hThread, dll);
    match result {
        Ok(()) => {
            // SAFETY: hThread is the child's suspended main thread.
            unsafe { ResumeThread(pi.hThread) };
            unsafe {
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
            }
            Ok(())
        }
        Err(e) => {
            // SAFETY: tear down the suspended child.
            unsafe {
                TerminateProcess(pi.hProcess, 1);
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
            }
            Err(e)
        }
    }
}

/// Write the DLL path into the target and queue LoadLibraryW on its main
/// thread. kernel32 is mapped at the same address in every process of a boot,
/// so the LoadLibraryW pointer we resolve here is valid in the child too.
fn queue_loadlibrary(process: HANDLE, thread: HANDLE, dll: &Path) -> Result<()> {
    let path_w = wide(dll);
    let bytes = std::mem::size_of_val(&path_w[..]);

    // SAFETY: process is a valid handle with PROCESS_VM_* rights from CreateProcessW.
    let remote = unsafe {
        VirtualAllocEx(
            process,
            ptr::null(),
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return Err(format!("VirtualAllocEx failed (os error {})", last_error()));
    }

    // SAFETY: remote points at `bytes` writable bytes; path_w is `bytes` long.
    let wrote = unsafe {
        WriteProcessMemory(
            process,
            remote,
            path_w.as_ptr().cast(),
            bytes,
            ptr::null_mut(),
        )
    };
    if wrote == 0 {
        return Err(format!(
            "WriteProcessMemory failed (os error {})",
            last_error()
        ));
    }

    // Resolve LoadLibraryW in our own kernel32 -- same address in the child.
    // SAFETY: kernel32 is always loaded; the names are NUL-terminated literals.
    let k32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
    if k32.is_null() {
        return Err("GetModuleHandleW(kernel32) failed".into());
    }
    // SAFETY: ASCII name, NUL-terminated.
    let load = unsafe { GetProcAddress(k32, c"LoadLibraryW".as_ptr().cast()) };
    let load = load.ok_or("GetProcAddress(LoadLibraryW) failed")?;

    // QueueUserAPC's APC proc takes one ULONG_PTR arg -- here, the remote path.
    // SAFETY: `load` has the LoadLibraryW signature, compatible with an APC proc
    // taking one pointer-sized argument; `remote` is that argument.
    let apc: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(load) };
    let queued = unsafe { QueueUserAPC(Some(apc), thread, remote as usize) };
    if queued == 0 {
        return Err(format!("QueueUserAPC failed (os error {})", last_error()));
    }
    Ok(())
}

fn last_error() -> u32 {
    // SAFETY: trivially safe FFI call.
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn run_mode_installed_no_args() {
        assert_eq!(determine_mode(&[], true), RunMode::Inject);
    }

    #[test]
    fn run_mode_not_installed_no_args() {
        assert_eq!(determine_mode(&[], false), RunMode::Wizard);
    }

    #[test]
    fn run_mode_install_flag() {
        let args = vec!["--install".into()];
        assert_eq!(determine_mode(&args, false), RunMode::HeadlessInstall);
        assert_eq!(determine_mode(&args, true), RunMode::HeadlessInstall);
    }

    #[test]
    fn run_mode_uninstall_flag() {
        let args = vec!["--uninstall".into()];
        assert_eq!(determine_mode(&args, false), RunMode::Uninstall);
    }

    #[test]
    fn run_mode_inject_flags_from_build_dir() {
        let args = vec!["--dll".into(), "my.dll".into()];
        assert_eq!(determine_mode(&args, false), RunMode::Inject);
    }

    #[test]
    fn is_under_ember_dir_checks() {
        let local = Path::new(r"C:\Users\test\AppData\Local");
        let installed =
            PathBuf::from(r"C:\Users\test\AppData\Local\Ember\ember-launcher.exe");
        assert!(is_under_ember_dir(Some(&installed), Some(local)));

        let other = PathBuf::from(r"C:\Users\test\Downloads\ember-launcher.exe");
        assert!(!is_under_ember_dir(Some(&other), Some(local)));

        assert!(!is_under_ember_dir(None, Some(local)));
        assert!(!is_under_ember_dir(Some(&installed), None));
    }

    #[test]
    fn install_dir_construction() {
        let local = Path::new(r"C:\Users\test\AppData\Local");
        assert_eq!(
            install::install_dir_from(local),
            PathBuf::from(r"C:\Users\test\AppData\Local\Ember")
        );
    }

    #[test]
    fn shortcut_target_construction() {
        let dir = Path::new(r"C:\Users\test\AppData\Local\Ember");
        assert_eq!(
            install::shortcut_target(dir),
            PathBuf::from(r"C:\Users\test\AppData\Local\Ember\ember-launcher.exe")
        );
    }

    #[test]
    fn shortcut_icon_construction() {
        let local = Path::new(r"C:\Users\test\AppData\Local");
        let icon = install::shortcut_icon_location(local);
        assert_eq!(
            icon,
            r"C:\Users\test\AppData\Local\DiscordPhoenix\app\Discord.exe,0"
        );
    }

    #[test]
    fn options_to_actions() {
        let opts = install::InstallOptions { add_desktop: true };
        let actions = install::actions_from_options(&opts);
        assert!(actions.create_start_menu);
        assert!(actions.create_desktop);

        let opts = install::InstallOptions { add_desktop: false };
        let actions = install::actions_from_options(&opts);
        assert!(actions.create_start_menu);
        assert!(!actions.create_desktop);
    }

    #[test]
    fn ember_aumid_matches_expected() {
        assert_eq!(install::EMBER_AUMID, "com.thesupersupersigma.ember");
    }
}
