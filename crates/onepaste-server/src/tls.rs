//! Self-signed certificate: generated on first start (ECDSA P-256, valid for 100 years) and reused afterwards.
//! Fingerprint = SHA-256 of the certificate SubjectPublicKeyInfo DER (base64url, no padding).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct Material {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub fingerprint: String,
}

pub fn paths(data_dir: &Path) -> (PathBuf, PathBuf) {
    let dir = data_dir.join("tls");
    (dir.join("cert.pem"), dir.join("key.pem"))
}

pub use onepaste_core::pin::{fingerprint_of_cert_der, spki_of_cert_der};

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    B64.decode(body.as_bytes()).ok()
}

/// Computes the fingerprint of `cert.pem` (used by the `connect-code` subcommand).
pub fn fingerprint_of_pem_file(path: &Path) -> io::Result<String> {
    let pem = fs::read_to_string(path)?;
    let der = pem_to_der(&pem).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad certificate"))?;
    fingerprint_of_cert_der(&der).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad certificate"))
}

/// Writes a temporary file and syncs it. The private key gets mode 0600 on Unix.
fn write_temp(path: &Path, data: &[u8], secret: bool) -> io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut f = opts.open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

fn sync_dir(dir: &Path) {
    // Directory fsync is not supported on some platforms; a failure can be ignored
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Whether both files exist and the private key belongs to the certificate. A crash between the two renames can leave
/// a new certificate next to an old key, so existing files are never trusted just because they are there.
fn pair_is_valid(cert_path: &Path, key_path: &Path) -> bool {
    let (Ok(cert_pem), Ok(key_pem)) = (fs::read_to_string(cert_path), fs::read_to_string(key_path)) else {
        return false;
    };
    let Some(cert) = pem_to_der(&cert_pem) else { return false };
    let Some(spki) = spki_of_cert_der(&cert) else { return false };
    let Ok(key) = rcgen::KeyPair::from_pem(&key_pem) else { return false };
    spki.ends_with(key.public_key_raw())
}

pub fn load_or_create(data_dir: &Path) -> io::Result<Material> {
    let (cert_path, key_path) = paths(data_dir);
    if !pair_is_valid(&cert_path, &key_path) {
        let dir = cert_path.parent().unwrap_or(data_dir);
        fs::create_dir_all(dir)?;
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(to_io)?;
        let mut params =
            rcgen::CertificateParams::new(vec!["onepaste.local".to_string(), "localhost".to_string()]).map_err(to_io)?;
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2120, 1, 1);
        let cert = params.self_signed(&key).map_err(to_io)?;
        // Both temporary files are complete and synced before either name changes
        let (cert_tmp, key_tmp) = (cert_path.with_extension("tmp"), key_path.with_extension("tmp"));
        write_temp(&key_tmp, key.serialize_pem().as_bytes(), true)?;
        write_temp(&cert_tmp, cert.pem().as_bytes(), false)?;
        crate::crash::crash_point("tls_before_rename");
        fs::rename(&cert_tmp, &cert_path)?;
        crate::crash::crash_point("tls_between_files");
        fs::rename(&key_tmp, &key_path)?;
        sync_dir(dir);
    }
    let fingerprint = fingerprint_of_pem_file(&cert_path)?;
    Ok(Material { cert_path, key_path, fingerprint })
}

fn to_io(e: rcgen::Error) -> io::Error {
    io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_next_to_a_foreign_key_is_replaced_by_a_matching_pair() {
        let dir = std::env::temp_dir().join(format!("onepaste-tls-mismatch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = load_or_create(&dir).unwrap();
        // Put a key that does not belong to the certificate in place, as a crash between the renames could
        let other = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        fs::write(&first.key_path, other.serialize_pem()).unwrap();
        let second = load_or_create(&dir).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint, "a mismatched pair must be regenerated");
        assert!(pair_is_valid(&second.cert_path, &second.key_path));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generates_reuses_and_fingerprints_the_same_certificate() {
        let dir = std::env::temp_dir().join(format!("onepaste-tls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let a = load_or_create(&dir).unwrap();
        let b = load_or_create(&dir).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.fingerprint.len(), 43);
        let _ = fs::remove_dir_all(&dir);
    }
}
