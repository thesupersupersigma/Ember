//! `ember.dll` — injected into Discord Phoenix, runs inside the client.
//!
//! v1 job (decision 1.4): unlock the access gate so Phoenix opens for any
//! logged-in account. Task 01 mapped the gate to a guard that, when the access
//! decision returns 0 (denied), mounts the "not available for your account"
//! screen. We find the guard by signature (task 01's pattern) inside the live
//! `.text` and patch its branch so it always treats the account as allowed.
//!
//! Patch (task 01, build 0.1.0.2208): at `match + 0x0D` the bytes are
//! `84 C0 74 06` (`test al,al; jz +6`). We overwrite with `B0 01 90 90`
//! (`mov al,1; nop; nop`) — forces "allowed" AND leaves al=1, which the task-01
//! edge-case note says the downstream guard chain requires (a bare jz->nop would
//! leave al=0 and misbehave).
//!
//! Anti-tamper (task 03a, partial/static): no self-checksum found, but not ruled
//! out. So after patching we re-read the bytes a few seconds later; if Discord
//! silently restored them we log it. `.text` is R-X, so we VirtualProtect to RW,
//! write, restore protection, and FlushInstructionCache.

#![cfg(windows)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

use ember_sig::Pattern;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{
    CreateThread, GetCurrentProcess, GetCurrentProcessId, Sleep,
};
use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

// Not exported by the Win32_UI_Shell feature in windows-sys 0.59.
#[link(name = "shell32")]
extern "system" {
    fn SHGetPropertyStoreForWindow(
        hwnd: *mut c_void,
        riid: *const windows_sys::core::GUID,
        ppv: *mut *mut c_void,
    ) -> i32;
}
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    GWL_STYLE,
};

// v1 unlock (revised after the first live run): Phoenix has TWO access checks.
// Gate A (task 01: guard +0x58d4c47 -> decision +0x52447c9) shows the screen
// BEFORE load. Gate B (decision fn +0x49b47fb, TWO callers) re-checks AFTER load
// and bounced us back. Patching one guard branch only beat Gate A (homepage
// flashed, then Gate B closed it). The robust fix — confirmed against Sunbird's
// reference — is to patch the DECISION FUNCTION itself so every caller sees
// "allowed", and the allowed value is 2 (downstream does `cmp al, 2`), not 1.
//
// We anchor on the UNIQUE caller site (distinctive `...03; jne; call; cmp al,2;
// setne; ...`), then follow its rel32 call to the decision function and patch its
// first 3 bytes to `mov al,2 ; ret`.

/// Unique caller of the access decision. The `call` (E8) is at CALL_OFFSET.
const CALLER_SIG: &str =
    "83 79 ?? 03 75 0C E8 ?? ?? ?? ?? 3C 02 0F 95 C0 EB 02 31 C0";

/// Offset of the `E8 rel32` call to the decision fn within CALLER_SIG.
const CALL_OFFSET: usize = 0x06;

/// Bytes we expect the decision fn to START with (`push rbp; push rsi; push rdi`).
/// We refuse to patch if the prologue doesn't match.
const DECISION_PROLOGUE: [u8; 3] = [0x55, 0x56, 0x57];

/// The replacement written over the decision fn's start: `mov al, 2 ; ret`.
/// `al == 2` is the value Phoenix's downstream `cmp al, 2` treats as allowed.
const REPLACEMENT: [u8; 3] = [0xB0, 0x02, 0xC3];

/// How long to wait before the anti-tamper re-read check (ms).
const RECHECK_AFTER_MS: u32 = 5_000;

/// The AUMID Ember owns — the installer sets this on shortcuts and this DLL
/// sets it on the running process.  Both sides must use the same literal.
const EMBER_AUMID: &str = "com.thesupersupersigma.ember";

/// Original `SetCurrentProcessExplicitAppUserModelID` pointer saved from the IAT.
static REAL_SET_AUMID: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// DLL entry point. On attach we spin up a worker thread and return immediately —
/// no heavy work inside loader lock.
#[no_mangle]
pub extern "system" fn DllMain(_dll: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        // Set AUMID on the main thread BEFORE Discord creates any windows.
        // Safe: IAT hook is pure memory writes; shell32 is already loaded
        // (Discord imports it) so SetCurrentProcessExplicitAppUserModelID
        // won't trigger a DLL load.
        set_process_aumid();

        // SAFETY: standard thread spawn; worker does its own error handling.
        unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(worker),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
        }
    }
    1
}

extern "system" fn worker(_param: *mut c_void) -> u32 {
    // Process AUMID is now set from DllMain (main thread, before window creation).

    let rc = match apply_unlock() {
        Ok(site) => {
            log(&format!("ember: gate patched at {site:p}"));
            // Anti-tamper re-read (task 03a mitigation).
            // SAFETY: `site` points at REPLACEMENT.len() bytes we just wrote in
            // the main module's .text, which stays mapped for the process life.
            let reverted = unsafe {
                Sleep(RECHECK_AFTER_MS);
                std::slice::from_raw_parts(site, REPLACEMENT.len()) != REPLACEMENT
            };
            if reverted {
                log("ember: WARNING patch was reverted — Discord may self-heal \
                     its code; switch to a function-level hook");
            } else {
                log("ember: patch still in place after recheck — looks stable");
            }

            // Install the HTTP request hook and built-in plugins.
            match ember_plugin::install() {
                Ok(()) => log("ember: plugin system installed"),
                Err(e) => log(&format!("ember: plugin install failed: {e}")),
            }
            0
        }
        Err(e) => {
            log(&format!("ember: unlock failed: {e}"));
            1
        }
    };

    // Set AUMID on Discord's window via property store (timing-proof).
    set_window_aumid();
    rc
}

/// Find the gate guard in the main module's memory and patch its branch.
/// Returns a pointer to the patched bytes (for the recheck).
fn apply_unlock() -> Result<*mut u8, String> {
    // SAFETY: we read the main module's own mapped image. The base/size come from
    // GetModuleInformation, and every pointer stays within [base, base+size).
    unsafe {
        let (base, size) = main_module_range()?;
        // Scan only the executable .text section, not the whole ~800 MB image:
        // faster and avoids matching the signature's bytes inside data.
        let (text_off, text_len) = text_section(base, size)?;
        let text = std::slice::from_raw_parts(base.add(text_off), text_len);

        let pat = Pattern::parse(CALLER_SIG).map_err(|e| format!("bad signature: {e}"))?;
        let matches = pat.find_all(text);
        if matches.is_empty() {
            return Err("access-gate caller signature not found — build may differ \
                        from 0.1.0.2208"
                .into());
        }
        if matches.len() > 1 {
            // The caller sig is expected to be unique; if not, we don't guess.
            return Err(format!(
                "access-gate caller signature matched {} times (expected 1)",
                matches.len()
            ));
        }
        let caller = matches[0];

        // Follow the E8 rel32 call at CALL_OFFSET to the decision function.
        // In the mapped image, section offset == RVA, so text starts at text_off.
        let call_rva = (text_off + caller + CALL_OFFSET) as isize;
        let rel_bytes = std::slice::from_raw_parts(base.add(text_off + caller + CALL_OFFSET + 1), 4);
        let rel = i32::from_le_bytes([rel_bytes[0], rel_bytes[1], rel_bytes[2], rel_bytes[3]]) as isize;
        // target = addr of next insn (call_rva + 5) + rel
        let decision_rva = (call_rva + 5 + rel) as usize;

        let site = base.add(decision_rva) as *mut u8;
        let current = std::slice::from_raw_parts(site, DECISION_PROLOGUE.len());
        if current != DECISION_PROLOGUE {
            return Err(format!(
                "decision prologue mismatch at RVA +0x{decision_rva:x}: expected \
                 {DECISION_PROLOGUE:02X?}, found {current:02X?}"
            ));
        }

        log(&format!(
            "ember: caller @ RVA +0x{:x} -> decision fn RVA +0x{decision_rva:x}; \
             patching mov al,2; ret",
            text_off + caller
        ));
        write_code(site, &REPLACEMENT)?;
        Ok(site)
    }
}

/// Write `bytes` over read-execute code: flip to RWX, copy, restore, flush.
///
/// # Safety
/// `dst` must point at `bytes.len()` valid, mapped bytes of executable code in
/// this process.
unsafe fn write_code(dst: *mut u8, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let mut old: PAGE_PROTECTION_FLAGS = 0;
        if VirtualProtect(dst.cast(), bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return Err("VirtualProtect(RWX) failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        let mut discard: PAGE_PROTECTION_FLAGS = 0;
        VirtualProtect(dst.cast(), bytes.len(), old, &mut discard);
        FlushInstructionCache(GetCurrentProcess(), dst.cast(), bytes.len());
        Ok(())
    }
}

/// Base address and size of the process's main module (Discord.exe).
///
/// # Safety
/// Calls Win32 module APIs; safe to call from a loaded module in a live process.
unsafe fn main_module_range() -> Result<(*const u8, usize), String> {
    unsafe {
        // GetModuleHandleW(NULL) returns the main executable's base.
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return Err("GetModuleHandleW(NULL) failed".into());
        }
        let mut info: MODULEINFO = std::mem::zeroed();
        let ok = GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        );
        if ok == 0 {
            return Err("GetModuleInformation failed".into());
        }
        Ok((info.lpBaseOfDll as *const u8, info.SizeOfImage as usize))
    }
}

/// Locate the first executable, byte-code section (`.text`) in the mapped image,
/// returning (offset-from-base, length). Parses the PE headers in memory; no
/// external crate. Falls back to the whole image only if parsing fails.
///
/// # Safety
/// `base` must point at a mapped PE image of at least `size` bytes.
unsafe fn text_section(base: *const u8, size: usize) -> Result<(usize, usize), String> {
    unsafe {
        // DOS header: 'MZ', e_lfanew at 0x3C points to the PE header.
        let rd_u16 = |off: usize| -> u16 {
            u16::from_le_bytes([*base.add(off), *base.add(off + 1)])
        };
        let rd_u32 = |off: usize| -> u32 {
            u32::from_le_bytes([
                *base.add(off),
                *base.add(off + 1),
                *base.add(off + 2),
                *base.add(off + 3),
            ])
        };
        if size < 0x40 || rd_u16(0) != 0x5A4D {
            return Err("not a PE image (no MZ)".into());
        }
        let pe = rd_u32(0x3C) as usize;
        if pe + 0x18 > size || rd_u32(pe) != 0x0000_4550 {
            return Err("not a PE image (no PE signature)".into());
        }
        // COFF header at `pe+4`: NumberOfSections (u16 @ +6), SizeOfOptionalHeader (u16 @ +20).
        let num_sections = rd_u16(pe + 6) as usize;
        let opt_size = rd_u16(pe + 20) as usize;
        // Section table follows the optional header.
        let sect_tab = pe + 24 + opt_size;
        const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
        const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
        for i in 0..num_sections {
            let s = sect_tab + i * 40; // each IMAGE_SECTION_HEADER is 40 bytes
            if s + 40 > size {
                break;
            }
            let virt_size = rd_u32(s + 8) as usize;
            let virt_addr = rd_u32(s + 12) as usize;
            let chars = rd_u32(s + 36);
            if chars & (IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE) != 0 && virt_size > 0 {
                let len = virt_size.min(size.saturating_sub(virt_addr));
                return Ok((virt_addr, len));
            }
        }
        // Fallback: scan everything.
        Ok((0, size))
    }
}

// ── AUMID: process identity for taskbar grouping ───────────────────────

/// Set the process AUMID to Ember's own value.  Two-pronged:
///  1. IAT hook so Discord's own `SetCurrentProcessExplicitAppUserModelID`
///     call uses our AUMID instead of its own.
///  2. Direct call to cover the race where Discord already called before the
///     hook was installed.
/// Fail-safe: errors are logged, never fatal.
fn set_process_aumid() {
    let hook_result = unsafe { hook_iat_set_aumid() };
    match hook_result {
        Ok(()) => log("ember: AUMID IAT hook installed"),
        Err(e) => log(&format!("ember: AUMID IAT hook skipped ({e})")),
    }
    let wide: Vec<u16> = EMBER_AUMID.encode_utf16().chain(std::iter::once(0)).collect();
    let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if hr != 0 {
        log(&format!(
            "ember: SetCurrentProcessExplicitAppUserModelID = 0x{hr:08x}"
        ));
    }
}

/// IAT hook detour: substitutes `EMBER_AUMID` regardless of the caller's argument.
unsafe extern "system" fn set_aumid_detour(_app_id: *const u16) -> i32 {
    let wide: Vec<u16> = EMBER_AUMID.encode_utf16().chain(std::iter::once(0)).collect();
    let real = REAL_SET_AUMID.load(Ordering::Acquire);
    if real.is_null() {
        return -1;
    }
    let f: unsafe extern "system" fn(*const u16) -> i32 =
        unsafe { std::mem::transmute(real) };
    unsafe { f(wide.as_ptr()) }
}

/// Walk the main module's PE import table and hook
/// `SetCurrentProcessExplicitAppUserModelID` in shell32.dll so every caller
/// (including Discord itself) ends up setting our AUMID.
///
/// # Safety
/// Must be called in a live process whose main module is a valid PE32+ image.
unsafe fn hook_iat_set_aumid() -> Result<(), String> {
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as *const u8;
    if base.is_null() {
        return Err("GetModuleHandleW(null) failed".into());
    }
    unsafe {
        let rd_u16 = |off: usize| -> u16 {
            u16::from_le_bytes([*base.add(off), *base.add(off + 1)])
        };
        let rd_u32 = |off: usize| -> u32 {
            u32::from_le_bytes([
                *base.add(off),
                *base.add(off + 1),
                *base.add(off + 2),
                *base.add(off + 3),
            ])
        };

        if rd_u16(0) != 0x5A4D {
            return Err("no MZ".into());
        }
        let pe = rd_u32(0x3C) as usize;
        if rd_u32(pe) != 0x0000_4550 {
            return Err("no PE sig".into());
        }
        let opt = pe + 24;
        if rd_u16(opt) != 0x020B {
            return Err("not PE32+".into());
        }

        // Import directory (data directory index 1, at opt + 112 + 1*8)
        let import_rva = rd_u32(opt + 120) as usize;
        if import_rva == 0 {
            return Err("no import directory".into());
        }

        // Walk IMAGE_IMPORT_DESCRIPTORs (20 bytes each, null-terminated)
        let mut desc = import_rva;
        loop {
            let name_rva = rd_u32(desc + 12) as usize;
            if name_rva == 0 {
                break;
            }
            if ascii_eq_ci(base.add(name_rva), b"shell32.dll") {
                let oft_rva = rd_u32(desc) as usize;
                let ft_rva = rd_u32(desc + 16) as usize;
                // Some linkers leave OriginalFirstThunk zero.
                let names = if oft_rva != 0 { oft_rva } else { ft_rva };
                let mut i = 0usize;
                loop {
                    let entry = *(base.add(names + i * 8) as *const u64);
                    if entry == 0 {
                        break;
                    }
                    // Skip ordinal imports (bit 63 set).
                    if entry & (1u64 << 63) == 0 {
                        let fn_name = base.add(entry as usize + 2); // skip hint u16
                        if ascii_eq(
                            fn_name,
                            b"SetCurrentProcessExplicitAppUserModelID",
                        ) {
                            let iat_slot =
                                base.add(ft_rva + i * 8) as *mut usize;
                            REAL_SET_AUMID
                                .store(*iat_slot as *mut c_void, Ordering::Release);
                            let mut old: PAGE_PROTECTION_FLAGS = 0;
                            VirtualProtect(
                                iat_slot.cast(),
                                8,
                                PAGE_EXECUTE_READWRITE,
                                &mut old,
                            );
                            *iat_slot = set_aumid_detour as *const () as usize;
                            VirtualProtect(iat_slot.cast(), 8, old, &mut old);
                            return Ok(());
                        }
                    }
                    i += 1;
                }
                // shortcut: only hooks the main EXE's IAT, not loaded DLLs
                return Err("function not in shell32 IAT".into());
            }
            desc += 20;
        }
        Err("shell32.dll not in imports".into())
    }
}

/// Case-insensitive match of a null-terminated C string against `name`.
unsafe fn ascii_eq_ci(ptr: *const u8, name: &[u8]) -> bool {
    for (i, &b) in name.iter().enumerate() {
        if unsafe { (*ptr.add(i)).to_ascii_lowercase() } != b.to_ascii_lowercase() {
            return false;
        }
    }
    unsafe { *ptr.add(name.len()) == 0 }
}

/// Exact match of a null-terminated C string against `name`.
unsafe fn ascii_eq(ptr: *const u8, name: &[u8]) -> bool {
    for (i, &b) in name.iter().enumerate() {
        if unsafe { *ptr.add(i) } != b {
            return false;
        }
    }
    unsafe { *ptr.add(name.len()) == 0 }
}

// ── AUMID: window property store (timing-proof) ──────────────────────

/// Polling interval when waiting for Discord's main window (ms).
const WINDOW_POLL_MS: u32 = 500;
/// Maximum time to wait for the window (ms).
const WINDOW_TIMEOUT_MS: u32 = 20_000;

/// IID_IPropertyStore = {886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}
const IID_IPROPERTY_STORE: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x886D8EEB,
    data2: 0x8CF2,
    data3: 0x4446,
    data4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99],
};

/// PKEY_AppUserModel_ID: fmtid {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, pid 5.
#[repr(C)]
struct PropertyKey {
    fmtid: windows_sys::core::GUID,
    pid: u32,
}

const PKEY_APPUSERMODEL_ID: PropertyKey = PropertyKey {
    fmtid: windows_sys::core::GUID {
        data1: 0x9F4C2855,
        data2: 0x9F79,
        data3: 0x4B39,
        data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
    },
    pid: 5,
};

/// PROPVARIANT layout for VT_LPWSTR on x86-64 (24 bytes).
#[repr(C)]
struct PropVariantLpwstr {
    vt: u16,              // VT_LPWSTR = 31
    _reserved: [u16; 3],
    pwsz: *const u16,
    _pad: usize,
}

/// Set Ember's AUMID on Discord's main window via its IPropertyStore.
/// This works on an already-created window, unlike
/// `SetCurrentProcessExplicitAppUserModelID` which only affects future windows.
/// Belt-and-suspenders: the process-level AUMID + IAT hook are kept above.
fn set_window_aumid() {
    // SAFETY: all COM/shell calls are fail-safe; errors are logged, not fatal.
    unsafe {
        let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        // S_OK (0) or S_FALSE (1) are both fine; RPC_E_CHANGED_MODE means
        // another COM mode was already set, which also works for our call.
        if hr < 0 && hr != -2147417850 /* RPC_E_CHANGED_MODE */ {
            log(&format!("ember: window AUMID skipped (CoInitializeEx = 0x{hr:08x})"));
            return;
        }
        let need_uninit = hr >= 0; // only uninitialise if we successfully inited

        let hwnd = poll_for_main_window();
        if hwnd.is_null() {
            log("ember: window AUMID skipped (window not found within timeout)");
            if need_uninit { CoUninitialize(); }
            return;
        }

        let mut store: *mut c_void = std::ptr::null_mut();
        let hr = SHGetPropertyStoreForWindow(hwnd, &IID_IPROPERTY_STORE, &mut store);
        if hr != 0 || store.is_null() {
            log(&format!(
                "ember: window AUMID skipped (SHGetPropertyStoreForWindow = 0x{hr:08x})"
            ));
            if need_uninit { CoUninitialize(); }
            return;
        }

        let wide: Vec<u16> = EMBER_AUMID.encode_utf16().chain(std::iter::once(0)).collect();
        let pv = PropVariantLpwstr {
            vt: 31, // VT_LPWSTR
            _reserved: [0; 3],
            pwsz: wide.as_ptr(),
            _pad: 0,
        };

        // IPropertyStore vtable:
        // [0] QueryInterface [1] AddRef [2] Release
        // [3] GetCount [4] GetAt [5] GetValue [6] SetValue [7] Commit
        let vtable = *(store as *const *const usize);

        let set_value: unsafe extern "system" fn(
            *mut c_void,
            *const PropertyKey,
            *const PropVariantLpwstr,
        ) -> i32 = std::mem::transmute(*vtable.add(6));
        let hr = set_value(store, &PKEY_APPUSERMODEL_ID, &pv);

        if hr == 0 {
            let commit: unsafe extern "system" fn(*mut c_void) -> i32 =
                std::mem::transmute(*vtable.add(7));
            commit(store);
            log("ember: window AUMID set");
        } else {
            log(&format!("ember: window AUMID skipped (SetValue = 0x{hr:08x})"));
        }

        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));
        release(store);

        if need_uninit { CoUninitialize(); }
    }
}

/// Poll for up to `WINDOW_TIMEOUT_MS` for Discord's main top-level window.
fn poll_for_main_window() -> *mut c_void {
    let pid = unsafe { GetCurrentProcessId() };
    let mut elapsed = 0u32;
    while elapsed < WINDOW_TIMEOUT_MS {
        let hwnd = find_main_window(pid);
        if !hwnd.is_null() {
            return hwnd;
        }
        unsafe { Sleep(WINDOW_POLL_MS) };
        elapsed += WINDOW_POLL_MS;
    }
    std::ptr::null_mut()
}

/// Find the best candidate main window for `pid`: visible, titled,
/// preferring `WS_OVERLAPPEDWINDOW` style.
fn find_main_window(pid: u32) -> *mut c_void {
    struct Ctx {
        pid: u32,
        best: *mut c_void,
        best_has_overlapped: bool,
    }

    unsafe extern "system" fn cb(hwnd: *mut c_void, lparam: isize) -> i32 {
        unsafe {
            let ctx = &mut *(lparam as *mut Ctx);
            let mut wpid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut wpid);
            if wpid != ctx.pid || IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut title = [0u16; 2];
            if GetWindowTextW(hwnd, title.as_mut_ptr(), 2) == 0 {
                return 1; // skip untitled windows (tooltips, helpers)
            }
            const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let overlapped = (style & WS_OVERLAPPEDWINDOW) == WS_OVERLAPPEDWINDOW;
            if overlapped && !ctx.best_has_overlapped || ctx.best.is_null() {
                ctx.best = hwnd;
                ctx.best_has_overlapped = overlapped;
            }
            1
        }
    }

    let mut ctx = Ctx { pid, best: std::ptr::null_mut(), best_has_overlapped: false };
    unsafe { EnumWindows(Some(cb), &mut ctx as *mut Ctx as isize) };
    ctx.best
}

// ── Logging ────────────────────────────────────────────────────────────

/// Minimal logging to a file next to the DLL's working dir. We avoid pulling in a
/// logging crate for v1; a line in `%TEMP%\ember.log` is enough to see results.
fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut dir) = std::env::var("TEMP") {
        dir.push_str("\\ember.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&dir) {
            let _ = writeln!(f, "{msg}");
        }
    }
}
