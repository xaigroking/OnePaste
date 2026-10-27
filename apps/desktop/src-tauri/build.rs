include!("../../../tools/build_version.rs");

fn main() {
    emit_build_version();
    println!("cargo:rerun-if-changed=windows-app.manifest");
    // An explicit manifest: the program never asks for administrator rights.
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the Tauri build script");
}
