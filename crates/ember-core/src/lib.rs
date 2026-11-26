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

use ember_sig::Pattern;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcess, Sleep};

/// Task 01 signature. The guard prologue; the patch site is `match + PATCH_OFFSET`.
/// `56 48 83 EC 20 48 89 CE E8 ?? ?? ?? ?? 84 C0 74 06 ...`
const GATE_SIG: &str =
    "56 48 83 EC 20 48 89 CE E8 ?? ?? ?? ?? 84 C0 74 06 48 83 C4 20 5E C3 \
     48 8D 15 ?? ?? ?? ?? 48 89 F1 48 83 C4 20 5E E9 ?? ?? ?? ??";

/// Offset from the match start to the 4 bytes we rewrite (`84 C0 74 06`).
const PATCH_OFFSET: usize = 0x0D;

/// Offset from the match start to the `lea rdx,[rip+disp]` that loads the gate
/// screen builder's address. Used to disambiguate the 3 twin matches.
const LEA_OFFSET: usize = 0x17;

/// RVA of the gate SCREEN BUILDER (task 01). The real guard is the signature
/// match whose internal lea targets this. Build-specific but only used as a
/// relative disambiguator among matches, not as a patch address.
const BUILDER_RVA: usize = 0x595B3C0;

/// Expected original bytes at the patch site — we refuse to patch anything else.
const EXPECTED: [u8; 4] = [0x84, 0xC0, 0x74, 0x06];
/// The replacement: `mov al,1 ; nop ; nop`.
const REPLACEMENT: [u8; 4] = [0xB0, 0x01, 0x90, 0x90];

/// How long to wait before the anti-tamper re-read check (ms).
const RECHECK_AFTER_MS: u32 = 5_000;

/// DLL entry point. On attach we spin up a worker thread and return immediately —
/// no heavy work inside loader lock.
#[no_mangle]
pub extern "system" fn DllMain(_dll: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
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
    match apply_unlock() {
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
            0
        }
        Err(e) => {
            log(&format!("ember: unlock failed: {e}"));
            1
        }
    }
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

        let pat = Pattern::parse(GATE_SIG).map_err(|e| format!("bad signature: {e}"))?;
        let matches = pat.find_all(text);
        if matches.is_empty() {
            return Err("gate signature not found — build may differ from 0.1.0.2208".into());
        }

        // The prologue signature is NOT unique (3 twins in 0.1.0.2208). The real
        // gate guard is the one whose internal `lea rdx,[rip+disp]` at +LEA_OFFSET
        // points at the gate SCREEN BUILDER. Disambiguate by that control-flow fact
        // instead of a fragile absolute address.
        let mut chosen: Option<usize> = None;
        for &hit in &matches {
            // match RVA (in-memory) = text_off(=.text VA) + hit, since we scan the
            // mapped image where section offset == RVA.
            let match_rva = (text_off + hit) as isize;
            let lea_rva = match_rva + LEA_OFFSET as isize;
            // bytes at the lea: 48 8D 15 <disp32>
            let lea = std::slice::from_raw_parts(base.add(text_off + hit + LEA_OFFSET), 7);
            if lea[0] != 0x48 || lea[1] != 0x8D || lea[2] != 0x15 {
                continue;
            }
            let disp = i32::from_le_bytes([lea[3], lea[4], lea[5], lea[6]]) as isize;
            let target = lea_rva + 7 + disp;
            if target == BUILDER_RVA as isize {
                chosen = Some(hit);
                break;
            }
        }
        let hit = chosen.ok_or_else(|| {
            format!(
                "found {} signature match(es) but none point at the gate builder — build may differ from 0.1.0.2208",
                matches.len()
            )
        })?;

        let site = base.add(text_off + hit + PATCH_OFFSET) as *mut u8;
        let current = std::slice::from_raw_parts(site, EXPECTED.len());
        if current != EXPECTED {
            return Err(format!(
                "patch site mismatch: expected {EXPECTED:02X?}, found {current:02X?}"
            ));
        }

        log(&format!(
            "ember: {} signature match(es); patching the one at RVA +0x{:x}",
            matches.len(),
            text_off + hit
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
