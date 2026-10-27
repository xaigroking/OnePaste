//! Self-signed certificate: generated on first start (ECDSA P-256, valid for 100 years) and reused afterwards.
//! Fingerprint = SHA-256 of the certificate SubjectPublicKeyInfo DER (base64url, no padding).

use crate::util::{b64url_encode, sha256};
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

/// Reads one DER TLV and returns (the whole TLV, its content, the remaining bytes).
fn read_tlv(b: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let tag_len = 1;
    let first = *b.get(tag_len)?;
    let (len, hdr) = if first & 0x80 == 0 {
        (usize::from(first), tag_len + 1)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 {
            return None;
        }
        let mut l = 0usize;
        for i in 0..n {
            l = (l << 8) | usize::from(*b.get(tag_len + 1 + i)?);
        }
        (l, tag_len + 1 + n)
    };
    let end = hdr.checked_add(len)?;
    if end > b.len() {
        return None;
    }
    Some((&b[..end], &b[hdr..end], &b[end..]))
}

/// Extracts the complete SubjectPublicKeyInfo DER from a certificate DER.
pub fn spki_of_cert_der(cert: &[u8]) -> Option<&[u8]> {
    let (_, cert_body, _) = read_tlv(cert)?; // Certificate
    let (_, tbs, _) = read_tlv(cert_body)?; // TBSCertificate
    let mut rest = tbs;
    if *rest.first()? == 0xA0 {
        rest = read_tlv(rest)?.2; // [0] version
    }
    for _ in 0..5 {
        // serial number, signature algorithm, issuer, validity, subject
        rest = read_tlv(rest)?.2;
    }
    Some(read_tlv(rest)?.0)
}

pub fn fingerprint_of_cert_der(cert: &[u8]) -> Option<String> {
    spki_of_cert_der(cert).map(|s| b64url_encode(&sha256(s)))
}

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

/// Temporary file, fsync, rename. The private key gets mode 0600 on Unix.
fn write_atomic(path: &Path, data: &[u8], secret: bool) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if secret {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = secret;
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        // Directory fsync is not supported on some platforms; a failure can be ignored
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

pub fn load_or_create(data_dir: &Path) -> io::Result<Material> {
    let (cert_path, key_path) = paths(data_dir);
    if !(cert_path.exists() && key_path.exists()) {
        fs::create_dir_all(cert_path.parent().unwrap_or(data_dir))?;
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(to_io)?;
        let mut params =
            rcgen::CertificateParams::new(vec!["onepaste.local".to_string(), "localhost".to_string()]).map_err(to_io)?;
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2120, 1, 1);
        let cert = params.self_signed(&key).map_err(to_io)?;
        // The private key is written first. Only both files together count as valid, so a crash in between just regenerates them on the next start.
        write_atomic(&key_path, key.serialize_pem().as_bytes(), true)?;
        write_atomic(&cert_path, cert.pem().as_bytes(), false)?;
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
