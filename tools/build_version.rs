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
    // What the update check compares with: the release name without its `v`, or the package version for a developer
    // build, and the time this build was made (seconds since the epoch)
    let version = std::env::var("ONEPASTE_BUILD_TAG").ok().filter(|t| !t.is_empty()).map(|t| t.trim_start_matches('v').to_string()).unwrap_or(pkg);
    let built = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=ONEPASTE_UPDATE_VERSION={version}");
    println!("cargo:rustc-env=ONEPASTE_BUILD_UNIX={built}");
}
