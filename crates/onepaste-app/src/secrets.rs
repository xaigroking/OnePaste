//! Small, offline heuristics for newly copied text, not credential validation.
//! Labels: RFC 7468 sections 10/11, OpenSSL pem.h and OpenSSH sshkey.c.
//! Prefixes: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-authentication-to-github#githubs-token-formats
//! Matching partial keys and token-like examples is intentional; false positives and misses are possible.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    PrivateKeyMarker,
    GithubTokenPrefix,
}

impl Reason {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::PrivateKeyMarker => "private_key_marker",
            Self::GithubTokenPrefix => "github_token_prefix",
        }
    }
}

const PRIVATE_KEYS: [&str; 6] = [
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN ENCRYPTED PRIVATE KEY-----",
    "-----BEGIN RSA PRIVATE KEY-----",
    "-----BEGIN DSA PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
];
const GITHUB_PREFIXES: [&[u8]; 6] = [b"ghp_", b"github_pat_", b"gho_", b"ghu_", b"ghs_", b"ghr_"];

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Borrows the complete UTF-8 text, uses constant space and a fixed number of linear scans.
/// No matched text, offsets, hashes or other content-derived data leave this function.
pub(crate) fn detect(text: &str) -> Option<Reason> {
    if PRIVATE_KEYS.iter().any(|marker| text.contains(marker)) {
        return Some(Reason::PrivateKeyMarker);
    }
    let bytes = text.as_bytes();
    for (i, &byte) in bytes.iter().enumerate() {
        if byte != b'g' || (i > 0 && word(bytes[i - 1])) {
            continue;
        }
        let tail = &bytes[i..];
        if GITHUB_PREFIXES.iter().any(|p| tail.starts_with(p) && tail.get(p.len()).is_some_and(|b| word(*b))) {
            return Some(Reason::GithubTokenPrefix);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(prefix: &[u8]) -> String {
        format!("{}{}", std::str::from_utf8(prefix).unwrap(), "Synthetic_123".repeat(9))
    }

    #[test]
    fn all_six_private_key_starts_match_without_an_end_line() {
        for marker in PRIVATE_KEYS {
            for text in [marker.to_string(), format!("note\n{marker}\nnot-a-key"), format!("before\r\n{marker}\r\nafter"), format!("quoted: {marker} trailing")] {
                assert_eq!(detect(&text), Some(Reason::PrivateKeyMarker));
            }
            assert_eq!(detect(&marker.to_ascii_lowercase()), None);
        }
        for label in ["PUBLIC KEY", "RSA PUBLIC KEY", "CERTIFICATE", "CERTIFICATE REQUEST"] {
            assert_eq!(detect(&format!("-----BEGIN {label}-----\nsynthetic")), None);
        }
    }

    #[test]
    fn all_six_token_prefixes_match_in_complete_text_and_common_contexts() {
        for prefix in GITHUB_PREFIXES {
            let sample = token(prefix);
            for text in [sample.clone(), format!("{{\"credential\":\"{sample}\"}}"), format!("export AUTH={sample}"), format!("curl -H 'Authorization: Bearer {sample}'"), format!("line one\n{sample}\nlast"), format!("\u{4e2d}\u{6587}{sample}\u{1f600}")] {
                assert_eq!(detect(&text), Some(Reason::GithubTokenPrefix));
            }
            // One ASCII word byte suffices; there is no fixed total token length or checksum.
            for suffix in ["a", "9", "_"] {
                assert_eq!(detect(&format!("{}{suffix}", std::str::from_utf8(prefix).unwrap())), Some(Reason::GithubTokenPrefix));
            }
        }
    }

    #[test]
    fn token_boundaries_case_and_bare_prefixes_are_respected() {
        for prefix in GITHUB_PREFIXES {
            let p = std::str::from_utf8(prefix).unwrap();
            for suffix in ["", " ", "\n", ".", "-", "\u{4e2d}"] {
                assert_eq!(detect(&format!("{p}{suffix}")), None);
            }
            assert_eq!(detect(&token(prefix).to_ascii_uppercase()), None);
            for before in ["x", "Z", "4", "_"] {
                assert_eq!(detect(&format!("{before}{}", token(prefix))), None);
            }
            // A failed candidate must not stop scanning for a later one.
            assert_eq!(detect(&format!("x{}\n{}", token(prefix), token(prefix))), Some(Reason::GithubTokenPrefix));
        }
    }

    #[test]
    fn ordinary_values_empty_text_and_near_matches_are_not_secrets() {
        for text in ["", "order_01234567890123456789", "2026-10-29T12:00:00Z", "192.168.1.123", "token_expiry_seconds_configuration_value", "0123456789abcdefABCDEF0123456789", "eyJhbGciOiJub25lIn0.synthetic.signature", "\u{4e2d}\u{1f600}"] {
            assert_eq!(detect(text), None);
        }
        assert_eq!(detect(&"ghp github_pat gh_ -----BEGIN PRIVATE KEY---- ".repeat(10000)), None);
    }

    #[test]
    fn scans_near_the_eight_mebibyte_item_ceiling_including_the_tail() {
        let start = std::time::Instant::now();
        let mut sample = "\u{1f600}".repeat((8 * 1024 * 1024 - 1024) / 4);
        sample.push('\n');
        sample.push_str(&token(GITHUB_PREFIXES[1]));
        assert_eq!(detect(&sample), Some(Reason::GithubTokenPrefix));
        eprintln!("full-text scan: {} bytes, {:?}", sample.len(), start.elapsed());
        sample.truncate(8 * 1024 * 1024 - 1024);
        sample.push_str(PRIVATE_KEYS[5]);
        assert_eq!(detect(&sample), Some(Reason::PrivateKeyMarker));
    }
}
