# 04 — HTTP Transport Map (build 0.1.0.2208)

Desktop Phoenix routes all Discord REST API traffic through a single Rust trait
method: `ReqwestTransport::send(&self, req: TransportRequest) -> Pin<Box<dyn Future>>`.
Hooking this method intercepts every outgoing HTTP request before it reaches the
network.

## Locating Transport::send

**Anchor string**: `"a host file body needs the host's transport"` — a unique
error string inside `send` that guards against mobile host-file bodies reaching
the desktop transport. Confirmed unique in this build by IDA string search
(1 match in .rdata at RVA +0x162EFAF0).

**Discovery algorithm** (runtime, build-independent):

1. Scan .rdata for the anchor string bytes.
2. Scan .text for `lea reg, [rip+disp32]` instructions whose computed target
   equals the string's virtual address.
3. Look up .pdata (RUNTIME_FUNCTION table) to find the function containing the
   LEA — that function is `Transport::send`.

This mirrors the sunbird reference's `DesktopHttpTransport::locate` approach.

## Section layout

| Section  | RVA          | Virtual size | Notes                        |
|----------|-------------|-------------|------------------------------|
| .text    | 0x00001000  | 0x132C7376  | ~322 MB executable code      |
| .rdata   | 0x132C9000  | 0x04B988C8  | ~79 MB read-only data        |
| .pdata   | 0x1850C000  | 0x01733274  | ~24 MB unwind/exception info |

Image base: 0x140000000.

## Request in memory — RawTransportRequest (0xF0 bytes)

Layout from sunbird reference, `repr(C)` mirror:

| Offset | Size | Field     | Type            | Notes                            |
|--------|------|-----------|-----------------|----------------------------------|
| 0x00   | 0x60 | headers   | RawHeaderMap    | http::HeaderMap mirror           |
| 0x60   | 0x38 | body      | RawBody         | tag + Bytes payload              |
| 0x98   | 0x18 | url       | RawVec (String) | {cap, ptr, len} observed layout  |
| 0xB0   | 0x10 | timeout   | (opaque)        |                                  |
| 0xC0   | 0x18 | method    | RawMethod       | bytes[0] = tag index into table  |
| 0xD8   | 0x18 | extra     | (opaque)        |                                  |

**RawVec field order** (`{cap, ptr, len}`) is the layout observed by sunbird
against this desktop build. Rust's `Vec`/`String` has no guaranteed field order
without `repr(C)`, so this is build-specific.

**Method encoding**: `method.bytes[0]` is an index: 0=OPTIONS, 1=GET, 2=POST,
3=PUT, 4=DELETE, 5=HEAD, 6=TRACE, 7=CONNECT, 8=PATCH, 9=QUERY. Values 10+ are
extension methods (inline or heap-allocated string).

## Dropping a request

Set `url.len = 0`. The original `send` still executes, but `Url::parse("")`
fails, producing an error future. No bytes reach the network. The host's String
capacity and pointer stay intact, so its allocator cleans up normally. This is
the same mechanism sunbird uses.

## Return value — RawPinnedFuture (16 bytes)

`Pin<Box<dyn Future>>` is a fat pointer: `{data: *mut (), vtable: *const ()}`.
On Win64 Rust ABI with two pointer-sized fields, returned in RAX:RDX.

## Poll vtable (future work)

The `SendFuture::poll` vtable slots can be found by:
1. Inside `send`, locating the `lea rdx, [rip+vtable]` that loads the future's
   vtable as the second half of the returned fat pointer.
2. The vtable's fourth qword (offset +24) is `poll`.

Response interception requires swapping these vtable entries. Not needed for v1
(silent-typing only hooks outgoing requests).

## Confidence and open questions

- **High**: anchor string is unique and stable across nearby builds.
- **High**: Drop mechanism (zero url.len) is well-tested by sunbird.
- **Medium**: RawVec field order is observed, not guaranteed by Rust ABI. If a
  future build reorders `{ptr, cap, len}`, ember reads garbage URLs and silently
  passes all requests (safe failure mode — the filter won't match).
- **Open**: no anti-tamper observed near the transport code (consistent with
  task 03a findings). The .text section is R-X; the hook uses VirtualProtect.
- **Open**: host allocator discovery (needed for body Replace) is not yet
  implemented; sunbird locates `__rust_alloc` / `__rust_dealloc` through the
  `Box::pin` call in send.
