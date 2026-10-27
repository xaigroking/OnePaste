//! The signature a real Tauri signer makes must pass this program's check. The CI job `update-signing` makes a throwaway
//! key with the signer, signs a manifest with the release script, and runs this test with the file names in the
//! environment. Without them (an ordinary `cargo test`) the test does nothing.

use onepaste_app::update::{verify, UpdateError};

#[test]
fn a_manifest_signed_by_the_tauri_signer_passes_and_a_changed_one_does_not() {
    let (Ok(key), Ok(manifest), Ok(sig)) = (std::env::var("UPDATE_TEST_PUBKEY_FILE"), std::env::var("UPDATE_TEST_MANIFEST"), std::env::var("UPDATE_TEST_SIG")) else {
        eprintln!("skipped: the signer files are not given");
        return;
    };
    let key = std::fs::read_to_string(key).unwrap();
    let manifest = std::fs::read(manifest).unwrap();
    let sig = std::fs::read_to_string(sig).unwrap();
    assert_eq!(verify(&manifest, &sig, &key), Ok(()), "the signature of the real signer is accepted");
    let mut changed = manifest.clone();
    let last = changed.len() / 2;
    changed[last] ^= 1;
    assert_eq!(verify(&changed, &sig, &key), Err(UpdateError::BadSignature), "a changed manifest is refused");
}
