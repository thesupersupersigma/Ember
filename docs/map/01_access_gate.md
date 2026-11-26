# 01 — Phoenix access gate (Discord.exe, build 0.1.0.2208, x86_64)

## Summary (plain English)

Phoenix shows the "This app isn't available for your account" screen from
exactly one place. A tiny guard function (`sub_1458D4C47`) calls a decision
function (`sub_1452447C9`); if that decision returns **0** it mounts the gate
screen, otherwise it lets the app proceed. The decision is a *membership test*:
it looks up a fixed 16‑byte key in a map hanging off the route context (a
feature‑gate / allowlist style check) and returns a byte — **nonzero = allowed,
zero = denied**. The smallest unlock is to make that byte always nonzero; the
cleanest place is the guard's own `test al,al; jz` branch, which we can locate
by a stable masked signature anchored to the unique gate strings.

All findings are read‑only; nothing was written to the binary.

---

## 1. Gate strings

The gate text lives in **`.rdata`** as i18n table records: each record is
`{ptr, u64, value, key}` with the value string and its translation key stored
inline, back‑to‑back.

| Key (inline) | Value text | Value VA | Value RVA | File offset |
|---|---|---|---|---|
| `DESKTOP_ACCESS_GATE_TITLE` | `This app isn't available for your account` | `0x1554CAE38` | `+0x154CAE38` | `0x154C9838` |
| `DESKTOP_ACCESS_GATE_BODY` | `This client is only available for select users. Log out to use another account.` | `~0x1554CAE98` | `+0x154CAE98` | `~0x154C9898` |

Hard anchors (exact substring matches from the binary):

- `available for your account` @ `0x1554CAE47`
- `available for select users` @ `0x1554CAEAC`
- `Log out to use another account` @ `0x1554CAEC8`
- key `DESKTOP_ACCESS_GATE_TITLE` @ `0x1554CAE61`

The TITLE record struct starts at VA `0x1554CAE20` (RVA `+0x154CAE20`, file
`0x154C9820`). The screen builder also references a **"Log Out"** button
(`LOGOUT` / `access-gate-log-out`), matching the body text.

These `DESKTOP_ACCESS_GATE_*` keys are the most stable anchor in the whole
chain — they are plain ASCII in `.rdata` and are very unlikely to change
between builds.

---

## 2. The chain: string → screen → decision

```
sub_14595B3C0   gate SCREEN builder      RVA +0x595B3C0   file 0x595A9C0
   ^ referenced by exactly ONE caller (total xrefs = 1)
sub_1458D4C47   gate GUARD (the branch)  RVA +0x58D4C47   file 0x58D4247
sub_1452447C9   DECISION (returns byte)  RVA +0x52447C9   file 0x5243DC9
sub_1496DD9A0   generic hashbrown find   RVA +0x96DD9A0   file 0x96DCFA0  (~300 callers; not gate-specific)
```

### 2a. Gate screen builder — `sub_14595B3C0` (+0x595B3C0)

Assembles the gate UI. Confirmed by its string use: it translates
`DESKTOP_ACCESS_GATE_TITLE` (hash `0x6293D110E5FFBD12`) and
`DESKTOP_ACCESS_GATE_BODY` (hash `0xFCFA7824FC7207F2`) and builds the
"Log Out" (`access-gate-log-out`) button. This is the thing we must stop being
mounted — but it is downstream; we patch the decision, not the builder.

### 2b. Gate guard / decision site — `sub_1458D4C47` (+0x58D4C47)

This is the function whose branch gates Phoenix.

```c
__int64 sub_1458D4C47(__int64 ctx)        // ctx in rcx
{
    char ok = sub_1452447C9(ctx);         // the decision (byte)
    if (ok == 0)                          // 0 => denied
        return sub_145245DE8(ctx, sub_14595B3C0);  // mount gate screen (tail call)
    return ok;                            // nonzero => allowed, proceed
}
```

Disassembly (patch site is the `test`/`jz`):

```
1458d4c47  push rsi
1458d4c48  sub  rsp, 20h
1458d4c4c  mov  rsi, rcx
1458d4c4f  call sub_1452447C9        ; decision -> al
1458d4c54  test al, al               ; <-- PATCH SITE  (RVA +0x58D4C54, file 0x58D4254)
1458d4c56  jz   loc_1458D4C5E        ; al==0 (denied) -> gate
1458d4c58  add  rsp, 20h
1458d4c5c  pop  rsi
1458d4c5d  retn                      ; al!=0 (allowed) -> proceed, returns al
1458d4c5e  lea  rdx, sub_14595B3C0   ; gate builder
1458d4c65  mov  rcx, rsi
1458d4c68  add  rsp, 20h
1458d4c6c  pop  rsi
1458d4c6d  jmp  sub_145245DE8        ; mount gate screen
```

Full bytes of `sub_1458D4C47` (43 bytes):

```
56 48 83 EC 20 48 89 CE E8 75 FB 96 FF 84 C0 74 06
48 83 C4 20 5E C3 48 8D 15 5B 67 08 00 48 89 F1 48
83 C4 20 5E E9 76 11 97 FF
```

### 2c. Decision function — `sub_1452447C9` (+0x52447C9)

A dedicated shim with **exactly one caller** (`sub_1458D4C47`):

```c
char sub_1452447C9(__int64 ctx)           // ctx in rcx
{
    __int128 key = xmmword_154A28430;     // fixed 16-byte lookup key (see note)
    return sub_1496DD9A0(ctx + 0x4B8, &key);   // map membership test -> 0/1
}
```

Bytes (37): `48 83 EC 38 0F 10 05 5C 3C 7E 0F 48 8D 54 24 20 0F 29 02 48 81 C1 B8 04 00 00 E8 B8 91 49 04 90 48 83 C4 38 C3`

- **Signature/shape:** `fn(ctx: *RouteContext) -> u8` (fastcall, arg in `rcx`).
- **Returns:** a byte. **`!= 0` means allowed** (proceed to the app); **`== 0`
  means denied** (show the gate). This is the value the guard tests with
  `test al,al; jz`.
- **What it does:** loads a constant 16‑byte key and asks the map at
  `ctx+0x4B8` whether that key is present/enabled. `sub_1496DD9A0` is Rust's
  `hashbrown` SwissTable `find` (SSE control‑byte group scan: `pcmpeqb` /
  `pmovmskb` / `tzcnt`, with per‑entry key compare in `sub_147B9B650`). So the
  gate is effectively a feature‑gate / allowlist **membership check**.

> **Note on the key constant:** a 16‑byte constant key lives at `.rdata`
> `xmmword_154A28430` (RVA `+0x14A28430`, file `0x14A26E30`). It is a
> compile‑time constant identical in every copy of this build (a feature /
> experiment id), **not** user‑specific data. Per the task's secret‑handling
> rule it is referenced by address only; its value is not reproduced here and
> is not needed for the patch.

---

## 3. Caller / surrounding context

`sub_1458D4C47` has exactly one caller: a route‑guard chain at `0x1448310EB`
(inside the function that owns it). The chain has the shape:

```
... mov rcx, rsi ; call <guard> ; test al,al ; jnz <skip this screen> ; lea rdx,<screen> ; call <mount> ...
```

i.e. a sequence of guards, each returning a byte; a **nonzero** result makes the
router skip mounting that screen. This matches Sunbird's described shape
(`signed_in && access(...) != allowed`): the "signed in / logged out" half is
handled by the surrounding router (earlier guards), and `sub_1458D4C47` is the
`access(...)` half for signed‑in users. The gate screen (`sub_14595B3C0`) is
reached from **nowhere else** (its xref count is 1), so this guard is the whole
gate.

---

## 4. Proposed patch (NOT applied)

Goal: make the gate answer "allowed" for any logged‑in account.

### Why a bare branch‑NOP is wrong (edge case)

Just NOP‑ing the `jz` (`74 06` → `90 90`) is **incorrect**: a denied user would
fall through to `retn` with `al = 0`, and the caller's `test al,al; jnz skip`
treats `0` differently from a real allowed user's nonzero — it would *not* skip,
and could mount the wrong screen. The patch must leave `al` **nonzero** on the
proceed path.

### Option A — neutralize the guard branch (RECOMMENDED)

One in‑place 4‑byte patch at RVA `+0x58D4C54` (file `0x58D4254`):

```
84 C0 74 06   ->   B0 01 90 90
(test al,al; jz +6)  (mov al,1; nop; nop)
```

Effect: `al` is forced to `1`, the conditional jump is gone, execution falls to
`add rsp,20; pop rsi; ret` returning `1` (allowed) for everyone. The decision
call still runs; its result is simply overwritten. Same length, no relocation.

- **Pros:** single site; anchored by a highly distinctive signature (§5) that
  **fails safe** (if the shape changes on a new build the scan simply misses and
  patches nothing); does not depend on the shim's internals.
- **Cons:** depends on the guard keeping this exact `test/jz/epilogue` shape
  (handled by the signature).

### Option B — force the decision function to return allowed

Overwrite the entry of `sub_1452447C9` (RVA `+0x52447C9`, file `0x5243DC9`):

```
48 83 EC 38 ...   ->   B0 01 C3   (mov al,1; ret)
```

Remaining original bytes become dead (we `ret` before touching `rsp`, and no
push happened, so the stack stays balanced). Safe because `sub_1452447C9` has
**exactly one caller** (the gate guard) — zero collateral.

- **Pros:** matches the "force the decision function" approach literally; tiny.
- **Cons:** the shim prologue (`48 83 EC 38 …`) is less unique on its own, so
  it should be reached via the guard's `call` rel32 rather than scanned for
  directly.

**Recommendation: Option A.** It is the most `ember-sig`‑friendly (one pattern,
one in‑place write, no pointer following) and fails safe on build drift.

---

## 5. Candidate masked signatures (`ember-sig` `AA BB ?? CC` format)

### S1 — gate guard (primary; locates the Option‑A patch site)

```
56 48 83 EC 20 48 89 CE E8 ?? ?? ?? ?? 84 C0 74 06 48 83 C4 20 5E C3 48 8D 15 ?? ?? ?? ?? 48 89 F1 48 83 C4 20 5E E9 ?? ?? ?? ??
```

- Match lands at the function start (`sub_1458D4C47`). **Patch at match + 0x0D**
  (`84 C0 74 06` → `B0 01 90 90`).
- **Wildcards (3 × rel32):** bytes 9–12 = `call sub_1452447C9` displacement;
  bytes 25–28 = `lea rdx, sub_14595B3C0` displacement; bytes 39–42 =
  `jmp sub_145245DE8` displacement. All three are RIP‑relative offsets that
  change every build; everything else (prologue, `test/jz`, epilogue, opcodes)
  is stable. The 43‑byte shape is almost certainly unique in the image.

Tighter variant (if a shorter pattern is preferred), patch at match + 0x08:

```
48 89 CE E8 ?? ?? ?? ?? 84 C0 74 06 48 83 C4 20 5E C3
```

### S2 — decision shim (for Option B)

```
48 83 EC 38 0F 10 05 ?? ?? ?? ?? 48 8D 54 24 20 0F 29 02 48 81 C1 B8 04 00 00 E8 ?? ?? ?? ?? 90 48 83 C4 38 C3
```

- Wildcards: bytes 7–10 = `movups xmm0,[rip+key]` displacement (points at
  `xmmword_154A28430`); bytes 27–30 = `call sub_1496DD9A0` displacement. The
  `add rcx,0x4B8` (`48 81 C1 B8 04 00 00`) and the overall shape are stable.
  Less unique than S1 on its own; prefer reaching it via S1's `call` rel32.

### S3 — build/anchor confirmation (most stable of all)

Scan `.rdata` for the literal ASCII key bytes to confirm this is the right
build before patching:

```
44 45 53 4B 54 4F 50 5F 41 43 43 45 53 53 5F 47 41 54 45 5F 54 49 54 4C 45   ("DESKTOP_ACCESS_GATE_TITLE")
```

---

## 6. Confidence & open questions

- **High confidence** that `sub_1458D4C47` is *the* access gate: the gate screen
  builder `sub_14595B3C0` is reached from exactly one place, and that builder is
  uniquely identified by the `DESKTOP_ACCESS_GATE_*` strings and the
  `access-gate-log-out` button. The `test al,al; jz → mount gate` branch is
  unambiguous in the disassembly.
- **Allowed vs. denied polarity** (`nonzero = allowed`) is confirmed from the
  `jz`‑to‑gate direction, not assumed.
- **Inferred, not proven:** that the decision is a feature‑gate / allowlist
  *membership* test. `sub_1496DD9A0` is clearly a generic `hashbrown` lookup and
  the key is a fixed constant, but I did not trace where the map at `ctx+0x4B8`
  is populated, nor decode what the 16‑byte key identifies. It does not affect
  the patch (we bypass the check entirely).
- **Anti‑tamper / integrity:** none observed on this immediate path. The
  `security_cookie`‑style `v47 = -2` in the builder is ordinary MSVC `/GS` stack
  protection, not a checksum; the `xmmword` load in the shim is just the lookup
  key. I did **not** audit for broader/background integrity checks (e.g. the
  `DiscordHook*.dll` modules, or a periodic self‑check thread) — out of scope
  for a read‑only locate, worth a dedicated pass before shipping a live patch.
- **Build drift:** all four functions are compiler‑generated and unsymbolized,
  so their *addresses* will move every build. Their *byte shapes* (S1/S2) are
  stable apart from the wildcarded rel32s, and the `.rdata` string anchors (S3)
  are the most stable of all. This is exactly why `ember-sig` pattern matching
  (not a hardcoded address) is the right runtime strategy; the patterns fail
  safe (miss → no patch) rather than mispatching on an unexpected build.

### Address reference

| Thing | VA | RVA | File off | Section |
|---|---|---|---|---|
| Gate TITLE record | `0x1554CAE20` | `+0x154CAE20` | `0x154C9820` | `.rdata` |
| Gate TITLE value | `0x1554CAE38` | `+0x154CAE38` | `0x154C9838` | `.rdata` |
| Gate builder `sub_14595B3C0` | `0x14595B3C0` | `+0x595B3C0` | `0x595A9C0` | `.text` |
| Gate guard `sub_1458D4C47` | `0x1458D4C47` | `+0x58D4C47` | `0x58D4247` | `.text` |
| Patch site (`test;jz`) | `0x1458D4C54` | `+0x58D4C54` | `0x58D4254` | `.text` |
| Decision `sub_1452447C9` | `0x1452447C9` | `+0x52447C9` | `0x5243DC9` | `.text` |
| hashbrown find `sub_1496DD9A0` | `0x1496DD9A0` | `+0x96DD9A0` | `0x96DCFA0` | `.text` |
| Lookup key const | `0x154A28430` | `+0x14A28430` | `0x14A26E30` | `.rdata` |

Image base `0x140000000`. `.text` file delta −0xA00, `.rdata` file delta −0x1600.
