# 04 — Silent Typing Plugin

Prevents Discord from sending typing indicators to other users. When you type
in a channel, Discord normally sends `POST /channels/{id}/typing` every few
seconds. This plugin drops those requests before they reach the network.

## Hook design

Ember hooks `ReqwestTransport::send` — the single function through which all
Discord REST API traffic passes. The hook:

1. **Locates** `send` at DLL load time by scanning `.rdata` for the unique
   anchor string `"a host file body needs the host's transport"`, then finding
   the `lea` instruction in `.text` that references it, then looking up `.pdata`
   for the containing function.

2. **Installs** a 14-byte absolute indirect jump (`jmp [rip+0]` + 8-byte
   address) over `send`'s prologue. The original prologue bytes are saved in a
   VirtualAlloc'd trampoline with a jump back, so the original function can
   still be called.

3. **Intercepts** each outgoing request: reads the method tag and URL from the
   `RawTransportRequest` struct (0xF0 bytes, offsets documented in
   `docs/map/04_http_transport.md`), extracts the API path, and runs it through
   registered plugin filters.

4. **Drops** matching requests by zeroing `url.len` — the original `send` still
   runs, but `Url::parse("")` fails cleanly with no network I/O.

## Plugin API shape

```rust
pub enum RequestAction { Pass, Drop, Replace(Vec<u8>) }

pub struct RequestFilter {
    pub method:      Option<&'static str>,
    pub path_prefix: Option<&'static str>,
    pub path_suffix: Option<&'static str>,
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn author(&self) -> &str;
    fn register(&self, ctx: &mut PluginCtx);
}
```

Plugins register request hooks via `PluginCtx::on_request(filter, handler)`.
Filters are checked in Rust before invoking handlers — non-matching requests
have zero overhead.

## Silent-typing registration

```rust
ctx.on_request(
    RequestFilter {
        method: Some("POST"),
        path_prefix: Some("/channels/"),
        path_suffix: Some("/typing"),
    },
    |_req| RequestAction::Drop,
);
```

Matches `POST /channels/{any}/typing` and returns `Drop`.

## How to test live

1. Build: `cargo build --release -p ember-core -p ember-inject`
2. Launch: `target\release\ember-launcher.exe`
3. Open any DM or channel and start typing.
4. On another account (or in a different client), observe: **no typing indicator
   appears** for the ember-injected account.
5. Check `%TEMP%\ember.log` for:
   - `ember-plugin: Transport::send at RVA +0x...`
   - `ember-plugin: hooks installed, silent-typing active`

## Known limits

- **Request-only**: this hook intercepts outgoing requests. Response interception
  (needed for plugins that modify incoming data) requires additionally hooking
  `SendFuture::poll` vtable slots — planned for v2.
- **Body replacement** (`RequestAction::Replace`) is declared but not yet wired:
  it needs host allocator discovery to avoid allocator mismatch when the host
  frees our `Vec<u8>`.
- **RawVec field order** (`{cap, ptr, len}`) is observed against build
  0.1.0.2208, not guaranteed by Rust ABI. A future Discord update could reorder
  it; in that case the filter sees garbage URLs and passes everything (safe
  failure — no crash, just no filtering). The fix is to re-verify the layout.
- **Rust ABI**: the hook's calling convention assumes `send`'s Rust-ABI lowering
  for `(ptr, ptr) -> (ptr, ptr)` matches the platform convention on Win64. This
  holds for current rustc; a major ABI change would need adjustment.
