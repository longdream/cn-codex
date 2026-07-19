fn main() {
    // Keep Windows app manifest explicit so UAC installer detection does not
    // treat updater.exe as a privileged installer (os error 740).
    let windows =
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows/app.manifest"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
