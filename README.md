# Ember

[![Discord](https://img.shields.io/badge/discord-join-5865F2)](https://discord.gg/trvtVNWzaf)

Ember unlocks **Discord Phoenix** (Discord's native Rust client) on Windows x64
and runs it past its access gate, so it opens for any logged-in account.

> Phoenix is the ground-up Rust/GPUI rewrite of the Discord desktop app. Early
> builds ship gated ("This app isn't available for your account"). Ember removes
> that gate in memory — it never modifies Discord on disk, so a restart cleanly
> reverts and Discord's own updates are unaffected.

Ember is a clean-room implementation in Rust. It is not affiliated with Discord.

## How it works

- **`ember-launcher.exe`** starts `Discord.exe` suspended and forces `ember.dll`
  to load first (an APC-queued `LoadLibraryW`), getting Ember's code in before
  Discord's own code runs.
- **`ember.dll`** finds Discord's access-decision function in memory by a byte
  signature, verifies it, and patches it to always return "allowed". It writes
  only to the in-memory copy of `Discord.exe`.
- **`ember-sig`** is the signature scanner both pieces rely on.

Results are logged to `%TEMP%\ember.log`.

## Requirements

- Windows x64
- Rust (stable, MSVC toolchain) — install via <https://rustup.rs>
- Discord Phoenix installed (build `0.1.0.2208`; signatures are build-specific)

## Build

```
git clone <your-repo-url> ember
cd ember
cargo build --release -p ember-core -p ember-inject
```

Outputs land in `target\release\`:

- `ember-launcher.exe`
- `ember.dll`

Keep both files in the same folder.

> If a build ever reports `Finished` in a fraction of a second with no
> `Compiling` lines, cargo reused a stale cache. Run `cargo clean` first, then
> rebuild.

## Run

1. Fully quit Discord Phoenix (a second instance hands off to the running one).
2. From the folder holding both files:

   ```
   ember-launcher.exe
   ```

   Optional flags:

   ```
   ember-launcher.exe --exe "C:\path\to\Discord.exe"   # non-default install
   ember-launcher.exe --dll "C:\path\to\ember.dll"     # explicit DLL
   ```

3. Check `%TEMP%\ember.log`:
   - `patching mov al,2; ret` then `patch still in place after recheck` → unlocked.
   - `signature not found` → the installed build differs from `0.1.0.2208`; the
     signature must be re-derived for that build.

## Notes

- The patch is in memory only; closing Discord reverts it. Launch through
  `ember-launcher.exe` each time.
- Signatures are tied to build `0.1.0.2208`. A Discord update may move the target
  and require updated signatures.

## License

GPLv3. See [LICENSE](LICENSE).
