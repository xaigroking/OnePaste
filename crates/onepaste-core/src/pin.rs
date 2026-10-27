//! Certificate pinning helpers shared by the server (to publish its fingerprint) and the clients (to check it).
//! The fingerprint is the SHA-256 of the certificate SubjectPublicKeyInfo DER, encoded as base64url without padding.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use sha2::{Digest, Sha256};

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
    spki_of_cert_der(cert).map(|s| B64URL.encode(Sha256::digest(s)))
}
