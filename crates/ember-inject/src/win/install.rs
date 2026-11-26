use std::path::{Path, PathBuf};
use std::{env, fs};

use super::Result;

// ── Pure helpers (tested from mod.rs::tests) ────────────────────────────

pub fn install_dir_from(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Ember")
}

#[allow(dead_code)] // used by unit tests in mod.rs
pub fn shortcut_target(install_dir: &Path) -> PathBuf {
    install_dir.join("ember-launcher.exe")
}

pub fn shortcut_icon_location(local_appdata: &Path) -> String {
    let discord = local_appdata
        .join("DiscordPhoenix")
        .join("app")
        .join("Discord.exe");
    format!("{},0", discord.display())
}

// ── Options / actions ───────────────────────────────────────────────────

pub struct InstallOptions {
    pub pin_taskbar: bool,
    pub add_desktop: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            pin_taskbar: true,
            add_desktop: true,
        }
    }
}

#[derive(Debug)]
pub struct InstallActions {
    #[allow(dead_code)] // used by unit tests in mod.rs
    pub create_start_menu: bool,
    pub create_desktop: bool,
    pub attempt_pin: bool,
}

pub fn actions_from_options(opts: &InstallOptions) -> InstallActions {
    InstallActions {
        create_start_menu: true, // always
        create_desktop: opts.add_desktop,
        attempt_pin: opts.pin_taskbar,
    }
}

// ── Install ─────────────────────────────────────────────────────────────

fn local_appdata() -> Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA not set".into())
}

fn ember_dir() -> Result<PathBuf> {
    Ok(install_dir_from(&local_appdata()?))
}

/// Install: copy files, create shortcuts.
/// Returns `true` if taskbar pinning was attempted but failed.
pub fn do_install(opts: &InstallOptions) -> Result<bool> {
    let local = local_appdata()?;
    let dest = install_dir_from(&local);
    fs::create_dir_all(&dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;

    let me = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let my_dir = me.parent().ok_or("exe has no parent dir")?;

    // Copy ember-launcher.exe
    let dest_exe = dest.join("ember-launcher.exe");
    if !same_file(&me, &dest_exe) {
        fs::copy(&me, &dest_exe).map_err(|e| format!("copying launcher: {e}"))?;
    }

    // Copy ember.dll (must be next to the installer)
    let src_dll = my_dir.join("ember.dll");
    let dest_dll = dest.join("ember.dll");
    if src_dll.is_file() {
        if !same_file(&src_dll, &dest_dll) {
            fs::copy(&src_dll, &dest_dll).map_err(|e| format!("copying ember.dll: {e}"))?;
        }
    } else if !dest_dll.is_file() {
        return Err("ember.dll not found next to the installer".into());
    }

    // Create shortcuts
    let actions = actions_from_options(opts);
    create_shortcuts(&dest_exe, &local, actions.create_desktop)?;

    // Attempt taskbar pin (silently fail on modern Win10)
    let mut pin_failed = false;
    if actions.attempt_pin && pin_to_taskbar().is_err() {
        pin_failed = true;
    }

    Ok(pin_failed)
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.canonicalize()
        .ok()
        .zip(b.canonicalize().ok())
        .is_some_and(|(a, b)| a == b)
}

/// Create shortcuts via PowerShell `WScript.Shell`. Paths reach the script
/// through environment variables, never interpolated into the script text.
fn create_shortcuts(launcher: &Path, local_appdata: &Path, desktop: bool) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let icon = shortcut_icon_location(local_appdata);

    let dirs = if desktop {
        "@([Environment]::GetFolderPath('Programs'), \
         [Environment]::GetFolderPath('Desktop'))"
    } else {
        "@([Environment]::GetFolderPath('Programs'))"
    };
    let script = format!(
        "$w = New-Object -ComObject WScript.Shell; \
         foreach ($dir in {dirs}) {{ \
         $p = Join-Path $dir 'Ember.lnk'; \
         $s = $w.CreateShortcut($p); \
         $s.TargetPath = $env:EMBER_TARGET; \
         $s.IconLocation = $env:EMBER_ICON; \
         $s.Description = 'Discord (Ember)'; \
         $s.Save() }}"
    );

    let status = Command::new(super::powershell_path())
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env("EMBER_TARGET", launcher)
        .env("EMBER_ICON", &icon)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map_err(|e| format!("running powershell: {e}"))?;

    if !status.success() {
        return Err("creating shortcuts failed".into());
    }
    Ok(())
}

/// Try to pin the Start Menu shortcut to the taskbar using the shell verb.
/// shortcut: verb removed in Win10 1903+, silently fails on those builds
fn pin_to_taskbar() -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let script = "\
        $lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Ember.lnk'; \
        if (-not (Test-Path $lnk)) { exit 1 }; \
        $shell = New-Object -ComObject Shell.Application; \
        $dir = $shell.Namespace((Split-Path $lnk)); \
        $item = $dir.ParseName((Split-Path -Leaf $lnk)); \
        $verb = $item.Verbs() | Where-Object { $_.Name -match 'taskbar' }; \
        if ($verb) { $verb.DoIt() } else { exit 1 }";

    let status = Command::new(super::powershell_path())
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| format!("taskbar pin: {e}"))?;

    if !status.success() {
        return Err("pin verb not available".into());
    }
    Ok(())
}

/// Launch the installed ember-launcher.exe (detects installed location -> inject).
pub fn launch_installed() -> Result<()> {
    let dir = ember_dir()?;
    let exe = dir.join("ember-launcher.exe");
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("launching: {e}"))?;
    Ok(())
}

// ── Headless install / uninstall ────────────────────────────────────────

pub fn headless_install() -> Result<()> {
    let pin_failed = do_install(&InstallOptions::default())?;
    let msg = if pin_failed {
        "Ember installed. Taskbar pinning is not available on this \
         Windows version -- drag the Ember shortcut to the taskbar manually."
    } else {
        "Ember installed successfully."
    };
    super::info(msg);
    Ok(())
}

pub fn uninstall() -> Result<()> {
    remove_shortcuts();

    let dest = ember_dir()?;
    match fs::remove_dir_all(&dest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            super::alert(&format!(
                "Shortcuts removed, but {} could not be fully deleted ({e}).\n\
                 Quit Discord and delete it manually.",
                dest.display()
            ));
        }
        _ => {
            super::info("Ember has been uninstalled. Discord itself was not changed.");
        }
    }
    Ok(())
}

fn remove_shortcuts() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let script = "\
        foreach ($dir in @([Environment]::GetFolderPath('Programs'), \
        [Environment]::GetFolderPath('Desktop'))) { \
        $p = Join-Path $dir 'Ember.lnk'; \
        Remove-Item -LiteralPath $p -ErrorAction SilentlyContinue }";

    let _ = Command::new(super::powershell_path())
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .status();
}
