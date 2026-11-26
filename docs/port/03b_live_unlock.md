# 03b — Live unlock (ember-core + ember-inject)

> Plain-English: this is the first real "does it open?" step. `ember.exe` starts
> Discord Phoenix paused, slips `ember.dll` in, and lets it go. `ember.dll` finds
> the gate guard in Discord's code by signature, flips the one branch so every
> account is "allowed", then (5 s later) re-reads those bytes to check Discord
> didn't undo the patch. Results go to `%TEMP%\ember.log`.

## What was built (written in this session, needs compiling on Windows)
- `crates/ember-inject` -> `ember.exe`: suspended-start + APC LoadLibraryW injector.
  Flags: `--dll <path>`, `--exe <path>`, `-- <discord args>`. Defaults:
  Discord at `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe`, ember.dll beside
  ember.exe.
- `crates/ember-core` -> `ember.dll`: DllMain spawns a worker thread that:
  1. finds `.text` by parsing the PE headers in memory (scans code only),
  2. locates the gate guard with the task-01 signature (ember-sig),
  3. verifies the 4 bytes are `84 C0 74 06`, then writes `B0 01 90 90`
     (`mov al,1; nop; nop`) via VirtualProtect + FlushInstructionCache,
  4. after 5 s, re-reads the bytes and logs whether the patch held (task-03a
     anti-tamper mitigation).

## Build (Windows, x64)
    cd C:\Users\thesupersupersigma\Desktop\project5\ember
    cargo build --release -p ember-core -p ember-inject

Outputs:
    target\release\ember.exe
    target\release\ember.dll

## Run (first live attempt)
1. Fully quit Discord Phoenix (a second instance hands off to the running one).
2. Put `ember.exe` and `ember.dll` in the same folder (e.g. target\release).
3. Run `ember.exe`. It launches the default Phoenix with ember.dll injected.
   - Or point it explicitly:
     `ember.exe --exe "C:\...\ignored\DiscordPhoenix\app\Discord.exe"`
     (note: the ignored copy has a live token — prefer the real install path).
4. Watch `%TEMP%\ember.log`:
   - "gate patched at 0x..." then "patch still in place after recheck" = success.
   - "gate signature not found" = build differs from 0.1.0.2208, re-derive the
     signature (task 01) against the installed binary.
   - "patch site mismatch" = found the region but the bytes changed; re-check task 01.
   - "WARNING patch was reverted" = Discord self-heals; move to a function-level
     hook of the decision fn (+0x52447C9) instead of the in-place branch patch.

## Known limitations / TODO
- ember-sig scan is O(n*m); on ~322 MB .text the first scan may take a few
  seconds. The PERF note in ember-sig (skip-ahead on first fixed byte) removes this.
- No unload/restore path yet (v1 patches and leaves it; Discord restart reverts
  since we never touch the on-disk exe).
- Anti-tamper check is a single 5 s re-read; a periodic watcher could be added if
  the live test shows reversion.
- Not yet tested on a real run (written this session; compile + launch pending).

## Safety notes
- Only the in-memory copy of Discord.exe is modified; the on-disk file is never
  written, so a restart cleanly reverts and Discord's own updates are unaffected.
- ember.dll writes only the 4 verified bytes and refuses if they don't match.

## Build fixes (2026-11-26, first compile)
First `cargo build` failed with 2 hard errors + 32 warnings. Root causes:
- `CreateProcessW` and `CreateThread` are feature-gated behind `Win32_Security`
  in windows-sys 0.59 (their signatures take SECURITY_ATTRIBUTES). Added
  `"Win32_Security"` to both crates' windows-sys features. -> fixes both errors.
- 30 warnings were `unsafe_op_in_unsafe_fn` (edition-2024 style, our workspace
  lint): wrapped each unsafe-fn body in an explicit `unsafe {}` block in
  ember-core. Removed the unused `HANDLE` import there too.
- `.pdb` collision warning (ember.exe + ember.dll both emit ember.pdb): added a
  release profile with `strip = "debuginfo"` (we don't ship pdbs for v1).
