fn main() {
    println!("cargo:rerun-if-env-changed=ONEPASTE_BUILD_TAG");
    println!("cargo:rerun-if-changed=windows-app.manifest");
    // An explicit manifest: the program never asks for administrator rights.
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the Tauri build script");
}
