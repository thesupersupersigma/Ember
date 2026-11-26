# 07 — Gateway Event Dispatch Map (build 0.1.0.2208)

Desktop Phoenix receives WebSocket gateway events through a layered dispatch
chain. Each incoming message is parsed by a top-level handler that extracts the
opcode and event name, then delegates to domain-specific handlers that process
individual event types (MESSAGE_CREATE, VOICE_STATE_UPDATE, etc.).

## Dispatch architecture

This build does **not** use a simple swappable registry table (the {name_ptr,
name_len, handler_ptr} stride-24 format found in some references). Instead:

1. **Top-level handler** (sub_148020700, RVA +0x8020700, ~5 KB): parses the
   WebSocket frame, extracts fields `"op"`, `"t"` (with fallbacks `"type"`,
   `"evt"`, `"cmd"`) and `"s"`, switches on opcode.
2. **Domain handlers**: functions that process a subset of events using compiled
   match chains (string comparisons against packed event-name literals).
3. **Event names**: stored in stride-16 arrays {str_ptr, str_len} in .rdata;
   handler function RVAs in separate 4-byte tables. Accessed indirectly via
   base + index calculations (no direct xrefs to the RVA tables).

## Key addresses (IDA, image base 0x140000000)

| Symbol | VA | RVA | Notes |
|--------|-------------|-----------|-------|
| Gateway message handler | 0x148020700 | 0x8020700 | Top-level, ~5 KB. Refs "HEARTBEAT_ACK", "IDENTIFY" |
| Domain pre-filter | 0x14724FCA0 | 0x724FCA0 | Handles MESSAGE_CREATE, MESSAGE_ACK, CHANNEL_UNREAD_UPDATE, VOICE_STATE_UPDATE, CHANNEL_CREATE, etc. |
| Inner domain handler | 0x14723E460 | 0x723E460 | Called by pre-filter for further dispatch |
| Event name table | 0x155FFE9A0 | — | Stride-16 {str_ptr, str_len} entries |
| RVA table (thunks) | 0x157DBDD80 | — | 4-byte handler RVAs, no direct xrefs |
| RVA table (domain fns) | 0x157DBDDB8 | — | 4-byte handler RVAs, no direct xrefs |

## Domain handler calling convention

```
fn handler(
    state:    *const c_void,   // rcx — opaque dispatcher state
    name_ptr: *const u8,       // rdx — event name UTF-8 pointer
    name_len: usize,           // r8  — event name byte length
    a4:       *const c_void,   // r9  — unknown context
    a5:       *const c_void,   // stack — unknown context
    a6:       *const c_void,   // stack — unknown context
) -> i64                       // rax — 0 = no match / unhandled
```

Windows x64 ABI. The first three arguments carry the event name as a Rust
`&str`-like (pointer + length). Arguments 4–6 carry context/payload data whose
layout is not yet mapped.

## Hook target — domain pre-filter (sub_14724FCA0)

The domain pre-filter at RVA +0x724FCA0 is the hook target. It processes the
highest-traffic events (MESSAGE_CREATE, MESSAGE_ACK, etc.) and is identifiable
by a unique byte-pattern signature.

### Prologue (21 relocatable bytes)

```asm
41 56           push r14
56              push rsi
57              push rdi
53              push rbx
48 83 EC 48     sub rsp, 48h
4C 89 CE        mov rsi, r9        ; save a4
4C 89 C7        mov rdi, r8        ; save name_len
48 89 D3        mov rbx, rdx       ; save name_ptr
49 89 CE        mov r14, rcx       ; save state
C7 44 24 44 ..  mov [rsp+44h], 42  ; local var init
48 8D 0D ??..   lea rcx, [rip+...]  ; -> "MESSAGE_CREATE..." packed string
```

Bytes 0–20 (21 bytes) are safe to relocate: no RIP-relative addressing, no
branches. The 14-byte inline jump fits with 7 bytes to spare.

### Signature (ember-sig format)

```
41 56 56 57 53 48 83 EC 48
4C 89 CE 4C 89 C7 48 89 D3 49 89 CE
C7 44 24 44 2A 00 00 00
48 8D 0D ?? ?? ?? ??
4C 8D 44 24 44
BA 0E 00 00 00
```

46 bytes, 4 wildcards (LEA displacement). Anchored to the unique combination of:
prologue shape (4 pushes + sub rsp,48h), register save pattern, local init
value (42 = 0x2A), and `mov edx, 14` (length of "MESSAGE_CREATE").

### Verification

The signature match is verified against .pdata (RUNTIME_FUNCTION table) to
confirm the matched offset is a function start, not a mid-function coincidence.

## Hook mechanism

Same inline hook as the HTTP transport (task 04): 14-byte absolute jump
(`FF 25 00000000` + 8-byte address), with the original prologue bytes relocated
into an RWX trampoline. The detour reads only the event name (rdx + r8),
dispatches to registered plugin hooks, and calls through the trampoline for
unmodified events.

## Privacy constraints

The gateway detour reads **only** the event name string (arguments 2–3). It
never accesses payload data (arguments 4–6), message content, tokens or user
IDs. Debug logging (behind `cfg(debug_assertions)`) emits the event name and
whether a hook matched — nothing more.

## Confidence

- **High**: domain pre-filter identified through IDA decompilation and string
  references. Calling convention confirmed by register usage analysis.
- **High**: prologue is clean (no RIP-relative in first 21 bytes). Same inline
  hook mechanism as the proven HTTP transport hook.
- **Medium**: the register save pattern (which callee-saved registers hold which
  arguments) is build-specific. The signature wildcards only the LEA
  displacement; a build that reassigns registers will not match (safe: hook
  skipped, logged).
- **Low**: arguments 4–6 are unmapped. Replace(data) falls through to Pass in
  v1 until these are understood.

## Open questions

- Inner domain handler (sub_14723E460) processes additional events not covered
  by the pre-filter. A second hook could expand coverage.
- The top-level handler (sub_148020700) could be hooked instead for universal
  coverage, but its prologue contains RIP-relative instructions early, making
  inline hooking harder. A trampoline-based approach would need more prologue
  analysis.
- Event data layout (args 4–6) needs mapping to support EventAction::Replace.
