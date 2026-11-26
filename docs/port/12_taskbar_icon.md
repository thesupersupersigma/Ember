# Taskbar icon grouping

## Problem

Launching Discord via the Ember shortcut showed **two** taskbar icons: the
pinned shortcut and the running window. Windows groups them into one button
only when the shortcut's `System.AppUserModel.ID` matches the running
window's `AppUserModelID`.

Previous approach tried to discover Discord Phoenix's AUMID from its own
shortcut (with a Squirrel-convention fallback `com.squirrel.DiscordPhoenix.Discord`).
This failed because:

1. Phoenix is not a Squirrel app — the fallback was wrong.
2. Discord sets its **own** process AUMID at runtime via
   `SetCurrentProcessExplicitAppUserModelID`, overriding anything the
   shortcut tried to match.

## Design: both sides use Ember's own AUMID

Constant: `"com.thesupersupersigma.ember"`, defined in both `ember-core`
(DLL) and `ember-inject` (installer).

| Side | Where | Mechanism |
|------|-------|-----------|
| Shortcut | `install.rs` `set_shortcut_aumid` | COM `IPropertyStore::SetValue` via PowerShell/C# helper |
| Process  | `lib.rs` `set_process_aumid`      | IAT hook + direct Win32 call (belt-and-suspenders) |
| Window   | `lib.rs` `set_window_aumid`       | `SHGetPropertyStoreForWindow` + `IPropertyStore::SetValue` |

Because both sides set the same literal, they always match — no need to
discover or track Discord's AUMID.

## DLL timing (IAT hook)

`ember.dll` is loaded before Discord's `main()` via APC injection. Its
worker thread calls `set_process_aumid()` first, which does two things:

1. **IAT hook** on `SetCurrentProcessExplicitAppUserModelID` in the main
   module's import table. When Discord subsequently calls this function, the
   hook replaces Discord's argument with Ember's AUMID and calls through.
2. **Direct call** to `SetCurrentProcessExplicitAppUserModelID` with Ember's
   AUMID, covering the race where Discord already called before the hook was
   installed.

Both steps are fail-safe: errors are logged to `%TEMP%\ember.log` and never
prevent the unlock or crash Discord.

## Window property store (attempt 3)

The process-level AUMID (`SetCurrentProcessExplicitAppUserModelID`) only
affects windows created **after** the call. Because `ember.dll` is injected
into an already-running process whose main window may already exist, the
process AUMID arrives too late.

`set_window_aumid()` fixes this by setting the AUMID directly on the
concrete HWND via its property store:

1. Polls with `EnumWindows` (500 ms intervals, 20 s timeout) for a visible,
   titled, `WS_OVERLAPPEDWINDOW`-style window belonging to this process.
2. Calls `SHGetPropertyStoreForWindow` to get the window's `IPropertyStore`.
3. Sets `PKEY_AppUserModel_ID` (fmtid `{9F4C2855-...}`, pid 5) to Ember's
   AUMID via `SetValue` + `Commit`, using raw COM vtable calls (windows-sys
   does not expose `IPropertyStore` as a Rust interface).
4. Releases the store and uninitialises COM.

This runs on the worker thread after the unlock and plugin install, so it
never delays the gate patch. All errors are logged and swallowed.

The process-level IAT hook + direct call are kept as belt-and-suspenders:
they handle any windows Discord creates **after** the hook is installed.

## Reliability — honest assessment

Three layers now set the same AUMID:

| Layer | Covers | Timing |
|-------|--------|--------|
| IAT hook | Future calls by Discord's main EXE | Before Discord's `main()` |
| Direct process AUMID | Future windows (if no later override) | Before Discord's `main()` |
| Window property store | The existing main window | After window creation (~0.5–20 s) |

**The window property store is the most robust layer.** It is timing-proof
(works on already-created windows) and cannot be overridden by a later
process-AUMID call, because the per-window property takes precedence over
the per-process one in the shell's grouping logic.

**Remaining risk:** Windows taskbar grouping also considers the executable
path. The pinned shortcut points at `ember-launcher.exe`, but the running
window belongs to `Discord.exe`. Even with matching AUMIDs, the shell
**may** treat them as separate buttons if it caches the exe-path association
from a previous non-Ember launch. In testing, the AUMID match is
sufficient for grouping on a clean pin, but a stale taskbar cache (from
a prior Discord-direct launch) can prevent grouping until the user unpins
and re-pins the Ember shortcut.

**If grouping still fails after re-pinning**, this is a cosmetic issue and
further effort is unlikely to pay off — the shell's internal grouping
heuristics are undocumented and version-dependent. Consider it "best
effort" and stop chasing it.

## Manual test

1. Close Discord completely (check system tray).
2. `cargo build --release -p ember-core -p ember-inject`
3. Run the installer (or `ember-launcher.exe --install`).
4. If the Ember shortcut was previously pinned, **unpin and re-pin** it
   (clears stale taskbar cache).
5. Launch Discord via the Ember shortcut.
6. Wait up to 20 seconds for the window AUMID to apply.
7. Verify: **one** taskbar icon, not two.
8. Check `%TEMP%\ember.log` for:
   - `AUMID IAT hook installed` — process-level hook active.
   - `window AUMID set` — window property store succeeded.
   - If `window AUMID skipped (...)` appears, the reason is logged.
9. If two icons persist despite `window AUMID set`, unpin the Ember shortcut,
   close Discord, re-pin, and launch again (clears the shell's exe-path
   cache).
