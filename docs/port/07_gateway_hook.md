# 07 — Gateway event hook (ember-plugin)

> Plain-English: incoming Discord gateway events (MESSAGE_CREATE, VOICE_STATE,
> etc.) now pass through ember's plugin system before Phoenix processes them.
> Plugins can observe events, drop them silently, or (in future) replace their
> data. Privacy: the hook reads only the event NAME — never payloads, message
> content, tokens or user IDs.

## What was built

### Gateway hook core (`crates/ember-plugin/src/lib.rs`)

- **`EventAction`** enum: `Pass`, `Drop`, `Replace(Vec<u8>)`. Replace falls
  through to Pass in v1 (data layout unmapped).
- **`EventFilter`** struct: matches by event name, case-insensitive.
- **`EventInfo`** struct: read-only view exposing only the event name.
- **`PluginCtx::on_event(filter, handler)`**: registers a gateway event hook,
  consistent with the existing `on_request` API.
- **`install_gateway_hook()`**: locates the domain handler by ember-sig byte
  pattern, verifies via .pdata, installs an inline hook. Non-fatal — logs and
  continues if the signature doesn't match.
- **`gateway_detour()`**: reads event name from args 2–3, runs registered hooks
  inside `catch_unwind`, calls original via trampoline. Drop returns 0 (no-op).
- **`safe_prologue_len()`** extended with two new instruction patterns:
  - REX + reg-to-reg MOV (`0x48..0x4F 0x89/0x8B ModRM>=0xC0`) — 3 bytes
  - `mov dword [rsp+disp8], imm32` (`C7 44 24 xx + imm32`) — 8 bytes
- **`install_inline_hook()`** return type generalized from `SendFn` to
  `*const ()` — callers transmute to the correct function type.

### Proof plugin (`crates/ember-plugin/src/plugins/event_observer.rs`)

Registers a `MESSAGE_CREATE` event hook that always returns `Pass`. Proves the
API works end-to-end without altering any event. Registered in `builtins()`.

### Tests (6 new, all passing)

- `event_filter_exact_match` — case-insensitive matching, rejects partial names
- `event_action_drop_variant` — Drop variant construction
- `event_action_replace_fallthrough` — Replace variant construction
- `safe_prologue_reg_to_reg_mov` — extended prologue parser handles the gateway
  handler's 29-byte prologue (21 relocatable + 8 stack-init bytes)
- `gateway_sig_parses` — GATEWAY_SIG is valid ember-sig syntax
- `filter_matches_message_create` / `filter_rejects_partial_name` (in plugin)

## Install flow (inside `install()`)

```
1. HTTP transport hook (existing — task 04)
2. Gateway event hook (new — non-fatal)
   a. Parse GATEWAY_SIG via ember-sig
   b. Scan .text for matches (expect exactly 1)
   c. Verify match is at function start via .pdata
   d. Install 14-byte inline hook, store trampoline
3. Register built-in plugins (silent-typing + event-observer)
```

The gateway hook is installed alongside the HTTP hook in the same `install()`
call from `ember-core`'s worker thread. If the gateway signature doesn't match
(different build), the hook is skipped with a log message — the HTTP hook and
plugins still work.

## Debug logging

Behind `cfg(debug_assertions)` only:
- Event name when a hook matches
- Nothing else — no payloads, no content, no user data

Release builds emit only:
- `"ember-plugin: gateway event hook active"` (success)
- `"ember-plugin: gateway hook skipped: <reason>"` (failure)

## Build

```
cargo build --release -p ember-plugin -p ember-core -p ember-inject
cargo test --workspace
```

## Limitations and future work

- **Replace** falls through to Pass — args 4–6 (payload data) are unmapped.
- Coverage: only events processed by the domain pre-filter (MESSAGE_CREATE,
  MESSAGE_ACK, CHANNEL_UNREAD_UPDATE, VOICE_STATE_UPDATE, CHANNEL_CREATE, etc.).
  The inner domain handler covers additional events but is not hooked.
- Signature is build-specific. The register save pattern may change between
  builds; a mismatch results in a safe skip (logged).
