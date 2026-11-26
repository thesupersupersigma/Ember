//! Embed the application manifest so Common Controls v6 (comctl32 v6) is loaded.
//! native-windows-gui uses GetWindowSubclass, a v6 export; without this manifest
//! the exe fails at startup with "GetWindowSubclass could not be located".

fn main() {
    #[cfg(windows)]
    {
        // Only the GUI launcher (Windows target) needs the manifest.
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("ember-launcher.exe.manifest");
        if let Err(e) = res.compile() {
            // Don't hard-fail the build on resource issues in odd toolchains;
            // print so it's visible.
            eprintln!("warning: embedding manifest failed: {e}");
        }
    }
}
