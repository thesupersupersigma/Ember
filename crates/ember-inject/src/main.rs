//! `ember.exe` — the Ember launcher.
//!
//! Decision 1.6: Discord.exe resolves its imports from System32 only, so a DLL
//! planted beside it is never loaded. We start Discord **suspended**, write the
//! path to `ember.dll` into it, queue `LoadLibraryW(that path)` as an APC on its
//! main thread, then resume — so our DLL loads before Discord's own code runs.
//!
//! Usage:
//!   ember.exe                     launch the default Phoenix install with ember.dll
//!   ember.exe --dll <path>        use a specific ember.dll
//!   ember.exe --exe <path>        use a specific Discord.exe
//!   ember.exe -- <args...>        pass the rest to Discord.exe
//!
//! Default paths (can be overridden):
//!   Discord.exe : %LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe
//!   ember.dll   : next to ember.exe
//!
//! This is Windows-only. On other targets it prints a notice and exits.

#![cfg_attr(windows, allow(clippy::missing_safety_doc))]

#[cfg(not(windows))]
fn main() {
    eprintln!("ember-inject only runs on Windows.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(e) = win::run() {
        eprintln!("ember: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod win {
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

    type Result<T> = std::result::Result<T, String>;

    /// Encode a Rust string/path as a NUL-terminated UTF-16 buffer.
    fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn run() -> Result<()> {
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
                    println!("usage: ember-launcher [--dll <ember.dll>] [--exe <Discord.exe>] [-- <discord args>]");
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

        inject_launch(&exe, &dll, &passthrough)?;
        println!("ember: launched {} with {}", exe.display(), dll.display());
        Ok(())
    }

    fn default_discord_exe() -> PathBuf {
        let local = env::var_os("LOCALAPPDATA").unwrap_or_default();
        PathBuf::from(local)
            .join("DiscordPhoenix")
            .join("app")
            .join("Discord.exe")
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
        // Build the command line: "exe" arg1 arg2 ...
        let mut cmdline = format!("\"{}\"", exe.display());
        for a in passthrough {
            cmdline.push(' ');
            cmdline.push_str(a);
        }
        let mut cmdline_w = wide(cmdline);

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
            return Err(format!("WriteProcessMemory failed (os error {})", last_error()));
        }

        // Resolve LoadLibraryW in our own kernel32 — same address in the child.
        // SAFETY: kernel32 is always loaded; the names are NUL-terminated literals.
        let k32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
        if k32.is_null() {
            return Err("GetModuleHandleW(kernel32) failed".into());
        }
        // SAFETY: ASCII name, NUL-terminated.
        let load = unsafe { GetProcAddress(k32, c"LoadLibraryW".as_ptr().cast()) };
        let load = load.ok_or("GetProcAddress(LoadLibraryW) failed")?;

        // QueueUserAPC's APC proc takes one ULONG_PTR arg — here, the remote path.
        // SAFETY: `load` has the LoadLibraryW signature, compatible with an APC proc
        // taking one pointer-sized argument; `remote` is that argument.
        let apc: unsafe extern "system" fn(usize) =
            unsafe { std::mem::transmute(load) };
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
}
