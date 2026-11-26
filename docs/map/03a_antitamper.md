# 03a — Anti-tamper audit (PARTIAL, static-only) — build 0.1.0.2208

> Plain-English summary: Discord's code section is read-only, so Ember must use
> VirtualProtect to make the 4 bytes writable, patch, then restore protection and
> flush the instruction cache. The DiscordHook* modules are an overlay/game-capture
> system (they hook OTHER games, not Discord itself), and DiscordSystemHelper does
> TLS cert verification + telemetry — neither watches Discord.exe's own memory. No
> positive evidence of self-integrity-checking was found, BUT this audit is
> static-only (PE headers, imports, strings, constant sweep); the IDA code-tracing
> that would turn this into a hard verdict was not done. Treat as YELLOW (leaning
> GREEN): proceed with VirtualProtect + a post-startup patch, but keep the live
> patch reversible and watch for the bytes being restored.

## Method & limitation
Done from a non-IDA environment: PE section/permission parse (pefile), import-table
scan for anti-tamper APIs, string classification of the helper modules, and a raw
byte-constant sweep for hash/CRC polynomials. NOT done: IDA xref tracing of the
SHA256/CRC call sites, confirming whether any of them hash the .text range, or
whether a thread re-reads code after load. Those remain open for an IDA pass.

## Item 1 — Self-checks on .text  (PARTIAL)
- Hash constants ARE present in Discord.exe: CRC32 poly (405), SHA256 init (29),
  SHA1 (12), FNV (21/6). This is expected for any large app (asset hashing, cache
  keys, hashbrown, update/network integrity) and is NOT by itself evidence of code
  self-checksumming.
- Not resolved statically: whether any site hashes the module's own .text range.
  OPEN for IDA.

## Item 2 — Hook modules  (ANSWERED)
- DiscordHook.dll / DiscordHook64.dll: OVERLAY / GAME-CAPTURE. Strings:
  wumpus-hook, gpu_markers.rs, D3D9/11/12 + Vulkan + Steam present hooks,
  swapchain, "present thread", "Origin overlay". They inject the Discord overlay
  into OTHER games. They do not reference watching Discord.exe's own memory.
- DiscordHookHelper(64).exe: helper processes for the above.
- DiscordSystemHelper.exe: telemetry (twox-hash, hdrhistogram, ETW
  'discord_game_perf') + TLS cert verification (WinVerifyTrust,
  CertVerifyCertificateChainPolicy). Certificate/download validation, not code
  integrity.
- discord_wer.dll: Windows Error Reporting.
- Verdict: none of these are a self-tamper watchdog. The alarming VirtualProtect /
  WriteProcessMemory / ReadProcessMemory imports belong to the overlay injector.

## Item 3 — Page protections  (ANSWERED)
- Discord.exe .text is R-X (va 0x1000, vsize 0x132c7376 ≈ 322 MB of code). NOT
  writable. => Ember MUST VirtualProtect the target page to RW (or RWX) before the
  4-byte write, restore it after, and FlushInstructionCache. No RWX section exists
  to exploit. (IPPCODE is the second, tiny R-X section — Intel IPP, irrelevant.)

## Item 4 — Tamper response  (OPEN)
- Cannot be determined statically. Requires IDA: if a checksum mismatch path
  exists, trace what it does (exit / restore / report).

## Item 5 — Detour safety  (PRELIMINARY)
- With no evidence of active restoration, the task-01 recommended in-place 4-byte
  patch at +0x58D4C54 (84 C0 74 06 -> B0 01 90 90) is viable, applied via
  VirtualProtect. A function-level hook of the decision fn (+0x52447C9) is the
  fallback if a live test shows the bytes being reverted.

## Confidence & open questions
- Confidence: medium on GREEN-ness. High confidence the helper modules are not
  watchdogs; medium-low that Discord.exe itself never self-checksums (not traced).
- MUST still do in IDA before trusting a long-running patch:
  1. Trace the SHA256/CRC sites — any over the module's own code range?
  2. Any thread that re-reads/validates .text after startup?
  3. The mismatch/failure path (item 4).
- Practical mitigation for the first live test (03b): after patching, re-read the
  4 bytes a few seconds later to confirm they stayed patched (detects silent
  restoration cheaply, without needing the full IDA trace first).
