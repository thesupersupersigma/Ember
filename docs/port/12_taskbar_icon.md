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
| Process  | `lib.rs` `set_process_aumid`      | IAT hook + direct Win32 call |

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

## Reliability

The IAT hook covers the main EXE's import table only. If Discord loads
`SetCurrentProcessExplicitAppUserModelID` dynamically (e.g. via
`GetProcAddress` from a child DLL), the hook misses it and the direct call
is the sole defence. In that case, grouping depends on our direct call
running after Discord's — likely given the worker thread starts very early,
but not guaranteed.

If grouping still fails after a Discord update, the next step would be to
inline-hook `SetCurrentProcessExplicitAppUserModelID` inside shell32.dll
itself (catches all callers regardless of module). The inline hook machinery
already exists in `ember-plugin`.

## Manual test

1. `cargo build --release -p ember-core -p ember-inject`
2. Run the installer (or `ember-launcher.exe --install`).
3. Pin the **Ember** shortcut to the taskbar.
4. Launch Discord via the Ember shortcut.
5. Verify: **one** taskbar icon, not two.
6. Check `%TEMP%\ember.log` for `AUMID IAT hook installed` or the skip
   reason.
