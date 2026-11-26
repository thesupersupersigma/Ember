//! `ember-plugin` — HTTP request hook and plugin API for Discord Phoenix.
//!
//! Locates Desktop Phoenix's `ReqwestTransport::send` by its unique anchor
//! string, installs an inline hook, and dispatches outgoing requests through
//! registered plugin filters.
//!
//! v1 ships one built-in plugin: **silent-typing** — drops
//! `POST /channels/*/typing` so other users never see the typing indicator.
//!
//! # ABI assumptions (build 0.1.0.2208)
//!
//! The request struct mirror (`RawTransportRequest`, 0xF0 bytes) follows the
//! layout observed by the sunbird reference against the same Phoenix desktop
//! build. `Transport::send` uses Rust ABI; on Win64 with simple pointer args
//! the lowering matches the platform convention.

#![cfg(windows)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
    PAGE_PROTECTION_FLAGS,
};
use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

// =========================================================================
// Public API
// =========================================================================

/// What a request hook tells the transport to do.
pub enum RequestAction {
    /// Forward the request unchanged.
    Pass,
    /// Silently drop the request (no network I/O).
    Drop,
    /// Replace the request body before sending.
    Replace(Vec<u8>),
}

/// Rust-side filter — non-matching requests never enter hook handlers.
pub struct RequestFilter {
    pub method: Option<&'static str>,
    pub path_prefix: Option<&'static str>,
    pub path_suffix: Option<&'static str>,
}

impl RequestFilter {
    /// Does `(method, path)` pass this filter? Query strings are stripped
    /// before prefix/suffix checks.
    pub fn matches(&self, method: &str, path: &str) -> bool {
        if let Some(m) = self.method {
            if !method.eq_ignore_ascii_case(m) {
                return false;
            }
        }
        let clean = path.split(['?', '#']).next().unwrap_or(path);
        if let Some(p) = self.path_prefix {
            if !clean.starts_with(p) {
                return false;
            }
        }
        if let Some(s) = self.path_suffix {
            if !clean.ends_with(s) {
                return false;
            }
        }
        true
    }
}

/// Read-only view of an intercepted request.
pub struct RequestInfo<'a> {
    pub method: &'a str,
    pub path: &'a str,
}

/// Registration context handed to [`Plugin::register`].
pub struct PluginCtx;

impl PluginCtx {
    /// Register a request hook that runs for every matching outgoing request.
    pub fn on_request<F>(&mut self, filter: RequestFilter, handler: F)
    where
        F: Fn(&RequestInfo) -> RequestAction + Send + Sync + 'static,
    {
        hooks().lock().unwrap().push(RegisteredHook {
            filter,
            handler: Box::new(handler),
        });
    }
}

/// A native Ember plugin.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn author(&self) -> &str;
    fn register(&self, ctx: &mut PluginCtx);
}

// =========================================================================
// Hook registry
// =========================================================================

struct RegisteredHook {
    filter: RequestFilter,
    handler: Box<dyn Fn(&RequestInfo) -> RequestAction + Send + Sync>,
}

fn hooks() -> &'static Mutex<Vec<RegisteredHook>> {
    static HOOKS: OnceLock<Mutex<Vec<RegisteredHook>>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(Vec::new()))
}

// =========================================================================
// ABI mirror — Desktop Phoenix x86_64, build 0.1.0.2208
// =========================================================================
// Layout from sunbird reference, verified with size assertions.
// shortcut: field order of RawVec ({cap,ptr,len}) is observed, not guaranteed
// by Rust ABI; upgrade if a build reorders it (crash / garbage URL).

#[repr(C)]
struct RawVec {
    cap: usize,
    ptr: *mut u8,
    len: usize,
}

#[repr(C)]
struct RawMethod {
    bytes: [u8; 24],
}

/// Mirror of `libdiscord/crates/http`'s `TransportRequest`.
#[repr(C)]
struct RawTransportRequest {
    _headers: [u8; 0x60], // RawHeaderMap
    _body: [u8; 0x38],    // RawBody
    url: RawVec,           // offset 0x98
    _timeout: [u8; 0x10],
    method: RawMethod,     // offset 0xC0
    _extra: [u8; 0x18],
}
const _: () = assert!(size_of::<RawTransportRequest>() == 0xF0);

/// Return value of `Transport::send`: `Pin<Box<dyn Future<Output = Result<Response>>>>`.
/// On Win64 Rust ABI this pair is returned in RAX:RDX.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawPinnedFuture {
    data: *mut c_void,
    vtable: *const c_void,
}

type SendFn = unsafe fn(*const c_void, *mut RawTransportRequest) -> RawPinnedFuture;

static TRAMPOLINE: OnceLock<SendFn> = OnceLock::new();

const METHODS: [&str; 10] = [
    "OPTIONS", "GET", "POST", "PUT", "DELETE", "HEAD", "TRACE", "CONNECT", "PATCH", "QUERY",
];

/// Unique string inside `ReqwestTransport::send` (host-file body guard).
const ANCHOR: &[u8] = b"a host file body needs the host's transport";

// =========================================================================
// install()
// =========================================================================

/// Locate the HTTP transport, hook `Transport::send`, register built-in
/// plugins. Safe to call once from the DLL worker thread. Returns Ok on
/// repeat calls if already installed.
pub fn install() -> Result<(), String> {
    if TRAMPOLINE.get().is_some() {
        return Ok(());
    }
    // SAFETY: reads the main module's mapped image; called from the worker
    // thread before Discord makes HTTP requests.
    unsafe {
        let (base, size) = main_module_range()?;
        let text = pe_section(base, size, SectionKind::Text)?;
        let rdata = pe_section(base, size, SectionKind::Rdata)?;

        // 1. Find the anchor string in .rdata
        let rdata_bytes = std::slice::from_raw_parts(base.add(rdata.0), rdata.1);
        let anchor_local = memfind(rdata_bytes, ANCHOR)
            .ok_or("transport anchor string not found in .rdata")?;
        let anchor_va = base.add(rdata.0 + anchor_local) as usize;

        // 2. Scan .text for a LEA that loads the anchor address
        let text_bytes = std::slice::from_raw_parts(base.add(text.0), text.1);
        let text_va = base as usize + text.0;
        let lea_local = find_lea_to(text_bytes, text_va, anchor_va)
            .ok_or("no LEA reference to transport anchor in .text")?;

        // 3. Look up .pdata to find the containing function (Transport::send)
        let pdata = pe_section(base, size, SectionKind::Pdata)?;
        let lea_rva = text.0 + lea_local;
        let (fn_begin, _fn_end) = pdata_lookup(base, pdata, lea_rva)?;
        let send_ptr = base.add(fn_begin) as *mut u8;

        log(&format!(
            "ember-plugin: Transport::send at RVA +0x{fn_begin:x} \
             (anchor LEA at +0x{lea_rva:x})"
        ));

        // 4. Install inline hook
        let tramp = install_inline_hook(send_ptr, send_detour as *const ())?;
        TRAMPOLINE
            .set(tramp)
            .map_err(|_| "hook already installed (race)".to_string())?;

        // 5. Register built-in plugins
        let mut ctx = PluginCtx;
        SilentTyping.register(&mut ctx);

        log("ember-plugin: hooks installed, silent-typing active");
        Ok(())
    }
}

// =========================================================================
// Send detour
// =========================================================================

unsafe fn send_detour(
    self_ptr: *const c_void,
    req: *mut RawTransportRequest,
) -> RawPinnedFuture {
    let tramp = TRAMPOLINE
        .get()
        .expect("detour called before trampoline set");

    if !req.is_null() {
        // A panic here would crash Discord — catch it.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let req_ref = unsafe { &mut *req };
            let method = read_method(&req_ref.method);
            let url = read_url(&req_ref.url);

            if let Some(path) = api_path(url) {
                match run_hooks(method, path) {
                    RequestAction::Pass => {}
                    RequestAction::Drop => {
                        // Zero url.len → Url::parse("") fails → no network I/O.
                        // The host's String capacity/pointer stay intact for cleanup.
                        req_ref.url.len = 0;
                    }
                    RequestAction::Replace(_body) => {
                        // TODO(v2): body replacement needs host allocator awareness
                    }
                }
            }
        }));
    }

    // SAFETY: forward to the original Transport::send via trampoline.
    unsafe { tramp(self_ptr, req) }
}

fn run_hooks(method: &str, path: &str) -> RequestAction {
    let Ok(hooks) = hooks().lock() else {
        return RequestAction::Pass;
    };
    let info = RequestInfo { method, path };
    for hook in hooks.iter() {
        if hook.filter.matches(method, path) {
            match (hook.handler)(&info) {
                RequestAction::Pass => continue,
                action => return action,
            }
        }
    }
    RequestAction::Pass
}

// =========================================================================
// Reading request fields
// =========================================================================

fn read_method(raw: &RawMethod) -> &'static str {
    METHODS
        .get(raw.bytes[0] as usize)
        .copied()
        .unwrap_or("UNKNOWN")
}

fn read_url(raw: &RawVec) -> &str {
    if raw.len == 0 || raw.ptr.is_null() || raw.len > 8192 {
        return "";
    }
    // SAFETY: the host's String is valid UTF-8 and lives for this call.
    let bytes = unsafe { std::slice::from_raw_parts(raw.ptr, raw.len) };
    std::str::from_utf8(bytes).unwrap_or("")
}

/// `/channels/123/typing` from `https://discord.com/api/v9/channels/123/typing`.
fn api_path(url: &str) -> Option<&str> {
    let idx = url.find("/api/v")?;
    let after = idx + "/api/v".len();
    let digit_end = url[after..]
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(url.len() - after);
    if !url.starts_with("https://") || digit_end == 0 {
        return None;
    }
    let rest = &url[after + digit_end..];
    rest.starts_with('/').then_some(rest)
}

// =========================================================================
// Silent-typing plugin
// =========================================================================

struct SilentTyping;

impl Plugin for SilentTyping {
    fn name(&self) -> &str {
        "silent-typing"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn author(&self) -> &str {
        "ember"
    }
    fn register(&self, ctx: &mut PluginCtx) {
        ctx.on_request(
            RequestFilter {
                method: Some("POST"),
                path_prefix: Some("/channels/"),
                path_suffix: Some("/typing"),
            },
            |_req| RequestAction::Drop,
        );
    }
}

// =========================================================================
// Inline hook
// =========================================================================

/// Install a 14-byte absolute jump at `target` redirecting to `detour`.
/// Returns a trampoline function pointer to the original code.
unsafe fn install_inline_hook(
    target: *mut u8,
    detour: *const (),
) -> Result<SendFn, String> {
    const JMP_LEN: usize = 14; // FF 25 00000000 + 8-byte addr

    // 1. Measure safe-to-relocate prologue (pushes, sub rsp, mov to stack)
    let code = unsafe { std::slice::from_raw_parts(target, 64) };
    let prol = safe_prologue_len(code, JMP_LEN)?;

    // 2. Allocate RWX page for the trampoline
    let tramp = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            prol + JMP_LEN,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        ) as *mut u8
    };
    if tramp.is_null() {
        return Err("VirtualAlloc for trampoline failed".into());
    }

    // 3. Copy saved prologue bytes into trampoline
    unsafe { std::ptr::copy_nonoverlapping(target, tramp, prol) };

    // 4. Append absolute jump back to target + prol
    unsafe { write_abs_jmp(tramp.add(prol), target as usize + prol) };

    // 5. Overwrite target prologue with jump to detour
    unsafe {
        let mut old: PAGE_PROTECTION_FLAGS = 0;
        if VirtualProtect(target.cast(), prol, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return Err("VirtualProtect for hook site failed".into());
        }
        write_abs_jmp(target, detour as usize);
        for i in JMP_LEN..prol {
            *target.add(i) = 0x90; // NOP leftover bytes
        }
        let mut tmp: PAGE_PROTECTION_FLAGS = 0;
        VirtualProtect(target.cast(), prol, old, &mut tmp);
        FlushInstructionCache(GetCurrentProcess(), target.cast(), prol);
    }

    log(&format!(
        "ember-plugin: inline hook installed ({prol} prologue bytes relocated)"
    ));
    Ok(unsafe { std::mem::transmute(tramp) })
}

/// Write `jmp qword ptr [rip+0]; dq target` — 14 bytes total.
unsafe fn write_abs_jmp(dst: *mut u8, target: usize) {
    unsafe {
        // FF 25 00 00 00 00 = jmp [rip+0]
        *dst = 0xFF;
        *dst.add(1) = 0x25;
        std::ptr::write_unaligned(dst.add(2) as *mut u32, 0u32);
        std::ptr::write_unaligned(dst.add(6) as *mut usize, target);
    }
}

/// Count bytes of prologue instructions safe to copy to a trampoline
/// (no RIP-relative addressing). Returns error if < `min` bytes are safe.
fn safe_prologue_len(code: &[u8], min: usize) -> Result<usize, String> {
    let mut pos = 0;
    while pos < code.len().min(min + 16) {
        match code[pos] {
            // push rax..rdi
            0x50..=0x57 => pos += 1,
            // REX.B push r8..r15
            0x41 if matches!(code.get(pos + 1), Some(0x50..=0x57)) => pos += 2,
            // sub rsp, imm8
            0x48 if code.get(pos + 1..pos + 3) == Some(&[0x83, 0xEC]) => pos += 4,
            // sub rsp, imm32
            0x48 if code.get(pos + 1..pos + 3) == Some(&[0x81, 0xEC]) => pos += 7,
            // mov [rsp+disp8], reg  (REX.W/REX.WR + 89 + mod=01 rm=100 + SIB=0x24)
            0x48 | 0x4C
                if code.get(pos + 1) == Some(&0x89)
                    && code.get(pos + 2).is_some_and(|b| b & 0xC7 == 0x44)
                    && code.get(pos + 3) == Some(&0x24) =>
            {
                pos += 5;
            }
            // mov [rsp+disp32], reg
            0x48 | 0x4C
                if code.get(pos + 1) == Some(&0x89)
                    && code.get(pos + 2).is_some_and(|b| b & 0xC7 == 0x84)
                    && code.get(pos + 3) == Some(&0x24) =>
            {
                pos += 8;
            }
            // lea rbp, [rsp+disp8]
            0x48 if code.get(pos + 1..pos + 4) == Some(&[0x8D, 0x6C, 0x24]) => pos += 5,
            // lea rbp, [rsp+disp32]
            0x48 if code.get(pos + 1..pos + 4) == Some(&[0x8D, 0xAC, 0x24]) => pos += 8,
            _ => break,
        }
    }
    if pos >= min {
        Ok(pos)
    } else {
        Err(format!(
            "unrecognized prologue at byte {pos} (0x{:02X}), need {min} safe bytes",
            code.get(pos).copied().unwrap_or(0)
        ))
    }
}

// =========================================================================
// Transport discovery
// =========================================================================

/// First occurrence of `needle` in `haystack`.
fn memfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Find a `lea reg, [rip+disp32]` whose computed target equals `target_va`.
fn find_lea_to(text: &[u8], text_va: usize, target_va: usize) -> Option<usize> {
    const RIP_MODRM: [u8; 8] = [0x05, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D];
    for i in 0..text.len().saturating_sub(7) {
        if (text[i] == 0x48 || text[i] == 0x4C)
            && text[i + 1] == 0x8D
            && RIP_MODRM.contains(&text[i + 2])
        {
            let disp =
                i32::from_le_bytes([text[i + 3], text[i + 4], text[i + 5], text[i + 6]]);
            let next_ip = text_va + i + 7;
            if let Some(computed) = next_ip.checked_add_signed(disp as isize) {
                if computed == target_va {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Binary-search .pdata (sorted `RUNTIME_FUNCTION` array) for the function
/// containing `rva`. Returns `(begin_rva, end_rva)`.
fn pdata_lookup(
    base: *const u8,
    pdata: (usize, usize),
    rva: usize,
) -> Result<(usize, usize), String> {
    let (off, len) = pdata;
    let count = len / 12;
    let rd32 = |i: usize, field: usize| -> u32 {
        let p = off + i * 12 + field * 4;
        // SAFETY: p is within the mapped PE image.
        unsafe {
            u32::from_le_bytes([
                *base.add(p),
                *base.add(p + 1),
                *base.add(p + 2),
                *base.add(p + 3),
            ])
        }
    };
    let target = rva as u32;
    let (mut lo, mut hi) = (0usize, count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let begin = rd32(mid, 0);
        let end = rd32(mid, 1);
        if end <= target {
            lo = mid + 1;
        } else if begin > target {
            hi = mid;
        } else {
            return Ok((begin as usize, end as usize));
        }
    }
    Err(format!("RVA +0x{rva:x} not in any .pdata entry"))
}

// =========================================================================
// PE section helpers
// =========================================================================

#[derive(Clone, Copy)]
enum SectionKind {
    Text,
    Rdata,
    Pdata,
}

/// Returns `(offset_from_base, virtual_size)` for the first matching section.
fn pe_section(
    base: *const u8,
    size: usize,
    kind: SectionKind,
) -> Result<(usize, usize), String> {
    // SAFETY: base..base+size is the main module's mapped image.
    unsafe {
        let rd16 = |o: usize| u16::from_le_bytes([*base.add(o), *base.add(o + 1)]);
        let rd32 = |o: usize| {
            u32::from_le_bytes([
                *base.add(o),
                *base.add(o + 1),
                *base.add(o + 2),
                *base.add(o + 3),
            ])
        };
        if size < 0x40 || rd16(0) != 0x5A4D {
            return Err("not a PE image".into());
        }
        let pe = rd32(0x3C) as usize;
        if pe + 0x18 > size || rd32(pe) != 0x0000_4550 {
            return Err("no PE signature".into());
        }
        let num = rd16(pe + 6) as usize;
        let opt = rd16(pe + 20) as usize;
        let tab = pe + 24 + opt;

        const CODE: u32 = 0x0000_0020;
        const EXEC: u32 = 0x2000_0000;
        const INIT_DATA: u32 = 0x0000_0040;

        for i in 0..num {
            let s = tab + i * 40;
            if s + 40 > size {
                break;
            }
            let name = std::slice::from_raw_parts(base.add(s), 8);
            let vsize = rd32(s + 8) as usize;
            let vaddr = rd32(s + 12) as usize;
            let chars = rd32(s + 36);

            let matched = match kind {
                SectionKind::Text => chars & (CODE | EXEC) != 0 && vsize > 0,
                SectionKind::Rdata => name.starts_with(b".rdata"),
                SectionKind::Pdata => name.starts_with(b".pdata") && chars & INIT_DATA != 0,
            };
            if matched {
                let len = vsize.min(size.saturating_sub(vaddr));
                return Ok((vaddr, len));
            }
        }
        let label = match kind {
            SectionKind::Text => ".text",
            SectionKind::Rdata => ".rdata",
            SectionKind::Pdata => ".pdata",
        };
        Err(format!("PE section {label} not found"))
    }
}

// =========================================================================
// Win32 helpers
// =========================================================================

unsafe fn main_module_range() -> Result<(*const u8, usize), String> {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return Err("GetModuleHandleW(NULL) failed".into());
        }
        let mut info: MODULEINFO = std::mem::zeroed();
        if GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        ) == 0
        {
            return Err("GetModuleInformation failed".into());
        }
        Ok((info.lpBaseOfDll as *const u8, info.SizeOfImage as usize))
    }
}

fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut dir) = std::env::var("TEMP") {
        dir.push_str("\\ember.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&dir)
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_silent_typing() {
        let filter = RequestFilter {
            method: Some("POST"),
            path_prefix: Some("/channels/"),
            path_suffix: Some("/typing"),
        };
        assert!(filter.matches("POST", "/channels/123456789/typing"));
        assert!(filter.matches("post", "/channels/999/typing"));
        assert!(!filter.matches("GET", "/channels/123/typing"));
        assert!(!filter.matches("POST", "/channels/123/messages"));
        assert!(!filter.matches("POST", "/users/@me/typing"));
    }

    #[test]
    fn filter_strips_query_string() {
        let filter = RequestFilter {
            method: None,
            path_prefix: Some("/channels/"),
            path_suffix: Some("/typing"),
        };
        assert!(filter.matches("POST", "/channels/123/typing?nonce=abc"));
        assert!(filter.matches("POST", "/channels/123/typing#frag"));
    }

    #[test]
    fn filter_pass_all() {
        let filter = RequestFilter {
            method: None,
            path_prefix: None,
            path_suffix: None,
        };
        assert!(filter.matches("GET", "/anything"));
    }

    #[test]
    fn api_path_extraction() {
        assert_eq!(
            api_path("https://discord.com/api/v9/channels/123/typing"),
            Some("/channels/123/typing")
        );
        assert_eq!(
            api_path("https://discord.com/api/v9/users/@me"),
            Some("/users/@me")
        );
        assert_eq!(
            api_path("https://discord.com/api/v10/guilds/1"),
            Some("/guilds/1")
        );
        assert_eq!(api_path("https://example.com/foo"), None);
        assert_eq!(api_path("http://discord.com/api/v9/foo"), None);
    }

    #[test]
    fn safe_prologue_typical_win64() {
        // push rbp; push rbx; push rsi; push rdi; push r12..r15; sub rsp, 0x48
        let code: &[u8] = &[
            0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48,
            0x83, 0xEC, 0x48,
        ];
        assert_eq!(safe_prologue_len(code, 14).unwrap(), 16);
    }

    #[test]
    fn safe_prologue_with_stack_saves() {
        // push rbx; sub rsp, 0x20; mov [rsp+0x30], rsi
        let code: &[u8] = &[
            0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0x89, 0x74, 0x24, 0x30, 0x48, 0x89, 0x7C,
            0x24, 0x38, 0xCC,
        ];
        assert_eq!(safe_prologue_len(code, 14).unwrap(), 15);
    }

    #[test]
    fn safe_prologue_too_short() {
        let code: &[u8] = &[0x55, 0x53, 0x56, 0x57, 0xE8, 0x00, 0x00, 0x00, 0x00];
        assert!(safe_prologue_len(code, 14).is_err());
    }

    #[test]
    fn request_struct_size() {
        assert_eq!(size_of::<RawTransportRequest>(), 0xF0);
        assert_eq!(size_of::<RawMethod>(), 0x18);
        assert_eq!(size_of::<RawVec>(), 0x18);
    }

    #[test]
    fn find_lea_in_code() {
        // Construct a fake code buffer with a lea rcx, [rip+disp32]
        let text_va: usize = 0x1000;
        let target_va: usize = 0x5000;
        // At offset 4: lea rcx, [rip + disp32]
        // disp32 = target_va - (text_va + 4 + 7) = 0x5000 - 0x100B = 0x3FF5
        let disp: i32 = (target_va as i64 - (text_va as i64 + 4 + 7)) as i32;
        let disp_bytes = disp.to_le_bytes();
        let mut code = vec![0x90; 20];
        code[4] = 0x48; // REX.W
        code[5] = 0x8D; // LEA
        code[6] = 0x0D; // ModRM: rcx, [rip+disp32]
        code[7] = disp_bytes[0];
        code[8] = disp_bytes[1];
        code[9] = disp_bytes[2];
        code[10] = disp_bytes[3];

        assert_eq!(find_lea_to(&code, text_va, target_va), Some(4));
        assert_eq!(find_lea_to(&code, text_va, 0x9999), None);
    }

    #[test]
    fn memfind_basic() {
        assert_eq!(memfind(b"hello world", b"world"), Some(6));
        assert_eq!(memfind(b"hello", b"xyz"), None);
        assert_eq!(memfind(b"aaa", b"aa"), Some(0));
    }
}
