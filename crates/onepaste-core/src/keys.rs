//! Key hierarchy: a random master key (MK) encrypts all data; the passphrase-derived KEK only wraps the MK.
//! Changing the passphrase only re-wraps the MK and never re-encrypts any item.

use crate::crypto::{self, aad, KdfParams, Key, MIN_SALT_LEN};
use crate::error::Error;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

pub const KEYPARAMS_V: u8 = 1;

/// The key parameter record stored on the server and locally: the salt is public, and the wrapped MK cannot be recovered without the passphrase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyParams {
    pub v: u8,
    pub salt: String,
    pub kdf: KdfParams,
    pub wrapped_mk: String,
    /// base64(`seal(MK, MKCHECK, empty plaintext)`): confirms that an MK in hand belongs to this vault. Old records do not have this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mk_check: Option<String>,
}

impl KeyParams {
    /// Created by the first device: random salt plus random MK; returns the parameter record and the MK.
    pub fn create(pass: &str, kdf: KdfParams) -> Result<(Self, Key), Error> {
        let mk = Key::random()?;
        let rec = Self::wrap(pass, kdf, &mk)?;
        Ok((rec, mk))
    }

    fn wrap(pass: &str, kdf: KdfParams, mk: &Key) -> Result<Self, Error> {
        let mut salt = [0u8; MIN_SALT_LEN];
        OsRng.try_fill_bytes(&mut salt).map_err(|_| Error::Rng)?;
        let kek = crypto::derive_key(pass, &salt, &kdf)?;
        let wrapped = crypto::seal(&kek, aad::MKWRAP, mk.as_bytes())?;
        let check = crypto::seal(mk, aad::MKCHECK, &[])?;
        Ok(KeyParams {
            v: KEYPARAMS_V,
            salt: B64.encode(salt),
            kdf,
            wrapped_mk: B64.encode(wrapped),
            mk_check: Some(B64.encode(check)),
        })
    }

    /// Recovers the MK with the passphrase. An AEAD failure means "wrong passphrase" and doubles as the passphrase check, so no real item has to be trial-decrypted.
    pub fn unlock(&self, pass: &str) -> Result<Key, Error> {
        if self.v != KEYPARAMS_V {
            return Err(Error::UnsupportedVersion(self.v));
        }
        let salt = B64.decode(&self.salt).map_err(|_| Error::MalformedEnvelope)?;
        let wrapped = B64.decode(&self.wrapped_mk).map_err(|_| Error::MalformedEnvelope)?;
        let kek = crypto::derive_key(pass, &salt, &self.kdf)?;
        let mk = crypto::open(&kek, aad::MKWRAP, &wrapped).map_err(|_| Error::WrongPassphrase)?;
        let arr: [u8; 32] = mk.try_into().map_err(|_| Error::MalformedEnvelope)?;
        Ok(Key::from_bytes(arr))
    }

    /// Changes the passphrase in O(1): re-wraps the same MK under a new salt; existing items stay untouched.
    pub fn change_passphrase(&self, old: &str, new: &str) -> Result<Self, Error> {
        let mk = self.unlock(old)?;
        Self::wrap(new, self.kdf, &mk)
    }

    /// Confirms that `mk` belongs to this vault. A record without `mk_check` is an error, not a pass.
    pub fn verify_mk(&self, mk: &Key) -> Result<(), Error> {
        let check = self.mk_check.as_ref().ok_or(Error::InvalidArgument("mk_check missing"))?;
        let sealed = B64.decode(check).map_err(|_| Error::MalformedEnvelope)?;
        crypto::open(mk, aad::MKCHECK, &sealed).map(|_| ()).map_err(|_| Error::WrongRecoveryKit)
    }

    /// Recovery after a forgotten passphrase: first confirm the MK belongs to this vault, and only then re-wrap it under the new passphrase. No record is produced on failure.
    pub fn reset_passphrase(&self, mk: &Key, new_pass: &str) -> Result<Self, Error> {
        self.verify_mk(mk)?;
        Self::wrap(new_pass, self.kdf, mk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KDF: KdfParams = KdfParams::MIN;

    #[test]
    fn create_unlock_and_wrong_passphrase() {
        let (rec, mk) = KeyParams::create("pw1", KDF).unwrap();
        assert_eq!(rec.unlock("pw1").unwrap(), mk);
        assert_eq!(rec.unlock("pw2").unwrap_err(), Error::WrongPassphrase);
    }

    #[test]
    fn change_passphrase_keeps_mk_and_invalidates_old() {
        let (rec, mk) = KeyParams::create("old", KDF).unwrap();
        let rec2 = rec.change_passphrase("old", "new").unwrap();
        assert_eq!(rec2.unlock("new").unwrap(), mk);
        assert_eq!(rec2.unlock("old").unwrap_err(), Error::WrongPassphrase);
        assert_ne!(rec.salt, rec2.salt);
    }

    #[test]
    fn rejects_unknown_version_and_weak_params() {
        let (mut rec, _) = KeyParams::create("pw", KDF).unwrap();
        rec.v = 9;
        assert_eq!(rec.unlock("pw").unwrap_err(), Error::UnsupportedVersion(9));
        let (mut rec, _) = KeyParams::create("pw", KDF).unwrap();
        rec.kdf.m_kib = 64; // downgrade attack: the server weakens the parameters
        assert_eq!(rec.unlock("pw").unwrap_err(), Error::WeakKdf);
    }

    #[test]
    fn record_json_has_no_plaintext_key_material() {
        let (rec, mk) = KeyParams::create("secret-passphrase-marker", KDF).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("secret-passphrase-marker"));
        assert!(!json.contains(&B64.encode(mk.as_bytes())));
    }

    #[test]
    fn create_emits_mk_check_and_old_records_still_parse() {
        let (rec, _) = KeyParams::create("pw", KDF).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"mk_check\""));
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut().unwrap().remove("mk_check");
        let old: KeyParams = serde_json::from_value(v).unwrap();
        assert_eq!(old.mk_check, None);
        assert!(!serde_json::to_string(&old).unwrap().contains("mk_check"));
        assert_eq!(old.verify_mk(&Key::random().unwrap()).unwrap_err(), Error::InvalidArgument("mk_check missing"));
    }

    #[test]
    fn change_passphrase_keeps_mk_check_valid() {
        let (rec, mk) = KeyParams::create("old", KDF).unwrap();
        let rec2 = rec.change_passphrase("old", "new").unwrap();
        assert!(rec2.mk_check.is_some());
        rec2.verify_mk(&mk).unwrap();
    }

    #[test]
    fn verify_mk_accepts_own_and_rejects_foreign() {
        let (rec, mk) = KeyParams::create("pw", KDF).unwrap();
        rec.verify_mk(&mk).unwrap();
        let (_, other) = KeyParams::create("pw", KDF).unwrap();
        assert_eq!(rec.verify_mk(&other).unwrap_err(), Error::WrongRecoveryKit);
    }

    #[test]
    fn reset_passphrase_requires_the_right_mk() {
        let (rec, mk) = KeyParams::create("forgotten", KDF).unwrap();
        let (_, foreign) = KeyParams::create("x", KDF).unwrap();
        assert_eq!(rec.reset_passphrase(&foreign, "new").unwrap_err(), Error::WrongRecoveryKit);
        let rec2 = rec.reset_passphrase(&mk, "new").unwrap();
        assert_eq!(rec2.unlock("new").unwrap(), mk);
        assert_eq!(rec2.unlock("forgotten").unwrap_err(), Error::WrongPassphrase);
        rec2.verify_mk(&mk).unwrap();
    }

    #[test]
    fn debug_output_never_shows_mk_bytes() {
        let (rec, mk) = KeyParams::create("pw", KDF).unwrap();
        let shown = format!("{mk:?} {rec:?} {:?}", Error::WrongRecoveryKit);
        assert!(!shown.contains(&B64.encode(mk.as_bytes())));
        assert!(!shown.contains(&format!("{:?}", mk.as_bytes())));
        assert!(!shown.contains(&crate::recovery::encode(&mk)));
    }
}
