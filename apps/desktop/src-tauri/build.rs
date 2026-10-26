fn main() {
    println!("cargo:rerun-if-env-changed=ONEPASTE_BUILD_TAG");
    tauri_build::build()
}
