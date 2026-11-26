# Installer wizard

Ember's launcher doubles as a double-click installer. Non-technical users set
Ember up with a 6-page wizard; power users can run `--install` or `--uninstall`
from the command line.

## Wizard flow

1. **Welcome** -- title and one-liner.
2. **How it works** -- explains that Discord must be opened through Ember.
3. **Terms** -- disclaimer; a checkbox gates the Next button.
4. **Options** -- "Add to desktop", checked by default.
5. **Installing** -- brief progress, auto-advances.
6. **Done** -- optional "Launch Ember now" checkbox, pin-from-Start-Menu tip, Finish button.

Back/Next navigation on every applicable page. Cancel or the X button closes the
wizard at any time.

## What Install does

1. Creates `%LOCALAPPDATA%\Ember\` and copies `ember-launcher.exe` + `ember.dll`
   into it.
2. Creates shortcuts via PowerShell (`WScript.Shell` COM object):
   - **Start Menu**: `Ember.lnk` in the user's Programs folder. Always created.
   - **Desktop**: `Ember.lnk` on the Desktop. Only if the option is checked.
   - Shortcut properties:
     - `TargetPath` = the installed `ember-launcher.exe`
     - `IconLocation` = `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe,0`
     - `Description` = `Discord (Ember)`
   - Paths reach the PowerShell script through environment variables, never
     interpolated into the script text.
3. Sets `System.AppUserModel.ID` on each Ember shortcut to Ember's own
   AUMID (`com.thesupersupersigma.ember`), so Windows groups the shortcut
   and the running window into one taskbar button. The DLL side sets the
   same value on the running process (see `docs/port/12_taskbar_icon.md`).
   This step is best-effort -- if it fails, shortcuts still work but may
   show a separate taskbar icon.
4. The install is idempotent: re-running it updates the copied files and
   overwrites the existing shortcuts (no duplicates).

## Run-mode detection rule

The exe decides what to do based on where it is running from and its CLI args:

| Condition | Mode |
|---|---|
| `--install` flag | Headless install (no GUI), same file copy + shortcuts |
| `--uninstall` flag | Remove shortcuts and `%LOCALAPPDATA%\Ember\` |
| Exe is under `%LOCALAPPDATA%\Ember\` | Inject + launch Discord (the normal job) |
| Any other CLI flags (`--dll`, `--exe`, etc.) | Inject + launch (power user) |
| None of the above | Show the installer wizard |

## Uninstall

`ember-launcher.exe --uninstall` removes the shortcuts (Start Menu + Desktop)
and deletes the `%LOCALAPPDATA%\Ember\` directory. If the directory cannot be
fully removed (e.g. ember.dll is mapped by a running Discord), the user is told
to close Discord and delete it manually. Discord itself is never touched.

## Known limitation

A process that launches `Discord.exe` directly (e.g. the auto-updater
relaunching, or a taskbar pin of Discord's own shortcut) will not be injected.
Only launches through the Ember shortcut (which runs `ember-launcher.exe`) apply
the unlock. This is by design: Ember injects fresh each launch and never
modifies Discord on disk.

## Manual test checklist

- [ ] Double-click `ember-launcher.exe` from a build directory -- wizard opens.
- [ ] Walk through all 6 pages with Back/Next; verify the agree checkbox gates Next on page 3.
- [ ] Complete the install; verify files appear in `%LOCALAPPDATA%\Ember\`.
- [ ] Check that Start Menu and Desktop shortcuts exist and point at the installed exe.
- [ ] Launch from the Start Menu shortcut -- Discord should open unlocked, with ONE taskbar icon (Ember + Discord grouped).
- [ ] Run the installed `ember-launcher.exe` directly -- should inject (no wizard).
- [ ] Run `ember-launcher.exe --uninstall` -- shortcuts and directory removed.
- [ ] Run the installer again (idempotent) -- no duplicate shortcuts.
- [ ] Run `ember-launcher.exe --install` from a terminal -- headless install, message box confirmation.
