// Shared by the build scripts of the programs (included with `include!`). It sets ONEPASTE_VERSION_LONG, for example
// `0.0.1 (v0.0.1-main.7)`: the package version and the build id. The release workflows pass the build id in
// ONEPASTE_BUILD_TAG; a developer build shows the short git commit instead.
fn emit_build_version() {
    println!("cargo:rerun-if-env-changed=ONEPASTE_BUILD_TAG");
    let tag = std::env::var("ONEPASTE_BUILD_TAG")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
            out.status.success().then(|| format!("dev-{}", String::from_utf8_lossy(&out.stdout).trim()))
        })
        .unwrap_or_else(|| "dev".to_string());
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=ONEPASTE_VERSION_LONG={pkg} ({tag})");
}
