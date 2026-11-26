# 09 — Request Body Access

## Overview

Ember's HTTP request hook now reads and replaces message bodies, enabling
plugins to transform outgoing message content before it reaches Discord's
servers.

## Design

### Reading

When `RawBody.tag == 1` (the `Body::Bytes` variant), the hook reads bytes
via `RawBytes.ptr` and `RawBytes.len`.  Any other tag value is treated as
no body — the hook passes an empty slice to plugins.

### Replacing

Replacement uses an allocator-isolated `bytes::Bytes` vtable, mirroring the
sunbird reference design:

1. Drop the old `Bytes` through its own vtable:
   `(*old_vtable).drop(data, ptr, len)`.
2. Wrap the new body `Vec<u8>` in an `Arc<Vec<u8>>`.
3. Install a custom `RawBytesVtable` whose `drop` decrements the Arc and
   whose `clone` increments it.
4. Set `body.tag = 1` and `body.bytes` to the new `RawBytes`.

This keeps the new data in Ember's allocator (the injected DLL's heap).
When Discord finishes with the request and drops the body, it calls our
vtable's `drop`, which safely frees the Arc.

### Host allocator (not yet resolved)

The vtable's `to_vec` returns an empty Vec because Ember has not yet located
`__rust_alloc` in `Discord.exe`.  This is safe for the normal request path
(reqwest reads bytes via ptr/len, not `to_vec`).

## Active plugins

- **clear-urls** — strips tracking query parameters (`utm_*`, `fbclid`,
  `gclid`, etc.) from URLs in message content.
- **send-timestamps** — replaces backtick-wrapped times like `` `3:00PM` ``
  with Discord `<t:UNIX:t>` timestamp tags.
- **text-replace** — applies user-defined find/replace rules from
  `%APPDATA%\ember\text_replace.txt` (one rule per line, `find=>replace`).
- **unindent** — strips common leading indentation from fenced code blocks.

All four match `POST /channels/*/messages` (message send).  Unindent also
matches `PATCH /channels/*` (message edit).  Each plugin parses the JSON
body, transforms the `"content"` field, and returns `Replace` only if the
content changed.  If JSON parsing fails, the plugin returns `Pass`.

## Testing live

1. **clear-urls** — send `https://example.com?utm_source=test`; the sent
   message should have `?utm_source=test` stripped.
2. **send-timestamps** — send `` `3:00PM` ``; the sent message should show
   a Discord relative timestamp.
3. **text-replace** — create `%APPDATA%\ember\text_replace.txt` with
   `hello=>hi`, then send "hello world"; it should arrive as "hi world".
4. **unindent** — paste an indented code block; the sent code should have
   common leading whitespace removed.

## Known limits

- `to_vec` vtable function returns empty: if the host calls `to_vec` on a
  replaced body (unlikely in the send path), the content is lost.
- Text-replace rules are loaded once at startup; restart Discord to reload.
- No PATCH hook for clear-urls or send-timestamps (edit messages not covered).
- Body replacement on requests with `body.tag != 1` (stream bodies) is not
  supported.
