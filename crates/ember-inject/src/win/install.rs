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
    pub add_desktop: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self { add_desktop: true }
    }
}

#[derive(Debug)]
pub struct InstallActions {
    #[allow(dead_code)] // used by unit tests in mod.rs
    pub create_start_menu: bool,
    pub create_desktop: bool,
}

pub fn actions_from_options(opts: &InstallOptions) -> InstallActions {
    InstallActions {
        create_start_menu: true, // always
        create_desktop: opts.add_desktop,
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

/// Install: copy files, create shortcuts, set AUMID for taskbar grouping.
pub fn do_install(opts: &InstallOptions) -> Result<()> {
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

    // Best-effort AUMID grouping — failure is not an install error
    set_shortcut_aumid(actions.create_desktop);

    Ok(())
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

/// Ember's AppUserModelID — must match the value ember.dll sets on the process.
pub const EMBER_AUMID: &str = "com.thesupersupersigma.ember";

/// Best-effort: set `System.AppUserModel.ID` on Ember shortcuts so Windows
/// groups the pinned shortcut and the running window into one taskbar button.
/// Uses COM `IPropertyStore` via a small C# helper compiled by `Add-Type`.
fn set_shortcut_aumid(desktop: bool) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let _ = Command::new(super::powershell_path())
        .args(["-NoProfile", "-NonInteractive", "-Command", AUMID_SCRIPT])
        .env("EMBER_AUMID", EMBER_AUMID)
        .env("EMBER_AUMID_DESKTOP", if desktop { "1" } else { "0" })
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status();
}

const AUMID_SCRIPT: &str = r#"try {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class LnkAumid {
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")] class ShellLink {}
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPropertyStore {
        [PreserveSig] int GetCount(out uint c);
        [PreserveSig] int GetAt(uint i, out PropertyKey k);
        [PreserveSig] int GetValue(ref PropertyKey k, out PropVariant v);
        [PreserveSig] int SetValue(ref PropertyKey k, ref PropVariant v);
        [PreserveSig] int Commit();
    }
    [ComImport, Guid("0000010b-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPersistFile {
        [PreserveSig] int GetClassID(out Guid classID);
        [PreserveSig] int IsDirty();
        [PreserveSig] int Load([MarshalAs(UnmanagedType.LPWStr)] string path, uint mode);
        [PreserveSig] int Save([MarshalAs(UnmanagedType.LPWStr)] string path,
                               [MarshalAs(UnmanagedType.Bool)] bool remember);
        [PreserveSig] int SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string path);
        [PreserveSig] int GetCurFile(out IntPtr ppszFileName);
    }
    [StructLayout(LayoutKind.Sequential, Pack=4)]
    struct PropertyKey { public Guid fmtid; public uint pid; }
    [StructLayout(LayoutKind.Sequential)]
    struct PropVariant {
        public ushort vt; ushort r1, r2, r3;
        public IntPtr p; IntPtr p2;
    }
    static PropertyKey PKEY = new PropertyKey {
        fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = 5
    };
    public static void Set(string lnkPath, string aumid) {
        object link = new ShellLink();
        try {
            ((IPersistFile)link).Load(lnkPath, 0);
            var store = (IPropertyStore)link;
            var val = new PropVariant { vt = 31, p = Marshal.StringToCoTaskMemUni(aumid) };
            store.SetValue(ref PKEY, ref val);
            store.Commit();
            ((IPersistFile)link).Save(lnkPath, true);
            Marshal.FreeCoTaskMem(val.p);
        } finally { Marshal.ReleaseComObject(link); }
    }
}
'@
$dirs = @([Environment]::GetFolderPath('Programs'))
if ($env:EMBER_AUMID_DESKTOP -eq '1') { $dirs += [Environment]::GetFolderPath('Desktop') }
foreach ($dir in $dirs) {
    $p = Join-Path $dir 'Ember.lnk'
    if (Test-Path $p) { [LnkAumid]::Set($p, $env:EMBER_AUMID) }
}
} catch {}"#;

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
    do_install(&InstallOptions::default())?;
    super::info("Ember installed successfully.");
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
