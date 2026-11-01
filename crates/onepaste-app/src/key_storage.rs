//! Application-owned encrypted key files. Constructing a store only inspects our own files; unlocking is explicit.
use crate::{AppError, KeyStore};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_core::crypto::{self, KdfParams, Key};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zeroize::Zeroizing;

pub const KEY_FILE: &str = "local-key.json";
const CONTEXT: &[u8] = b"onepaste/local-master-key/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    New,
    Prepared,
    Locked,
    Unlocked,
    Recovery,
}

#[derive(Serialize, Deserialize)]
struct Record {
    v: u8,
    salt: String,
    kdf: KdfParams,
    sealed_key: String,
}
struct Wrapping {
    key: Key,
    salt: String,
}
#[derive(Default)]
struct State {
    key: Option<Key>,
    wrapping: Option<Wrapping>,
}

pub struct EncryptedKeyStore {
    dir: PathBuf,
    state: Mutex<State>,
}
fn storage(_: impl std::fmt::Display) -> AppError {
    AppError::KeyStore("file".into())
}
fn format_error() -> AppError {
    AppError::KeyStore("format".into())
}

/// Even sidecars without their database count as existing data. Nothing here opens or modifies a database.
pub(crate) fn has_existing_data(dir: &Path) -> Result<bool, AppError> {
    for name in [
        "onepaste.db",
        "history.db",
        crate::settings::STATE_FILE_NAME,
    ] {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            if dir
                .join(format!("{name}{suffix}"))
                .try_exists()
                .map_err(storage)?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(crate) fn empty_setup(dir: &Path) -> bool {
    if dir.join("history.db").exists() || dir.join(crate::settings::STATE_FILE_NAME).exists() {
        return false;
    }
    onepaste_client::SqliteStore::open_read_only(&dir.join("onepaste.db"))
        .and_then(|s| s.is_empty_setup())
        .unwrap_or(false)
}

impl EncryptedKeyStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            state: Mutex::new(State::default()),
        }
    }
    fn wrapping(pass: &str) -> Result<Wrapping, AppError> {
        if pass.chars().count() < 8 || pass.len() > 4096 {
            return Err(AppError::PassphraseRequired);
        }
        let random = Key::random().map_err(storage)?;
        let salt = &random.as_bytes()[..16];
        let key = crypto::derive_key(pass, salt, &KdfParams::DEFAULT).map_err(storage)?;
        Ok(Wrapping {
            key,
            salt: B64.encode(salt),
        })
    }
    fn read(&self) -> Result<Record, AppError> {
        let path = self.dir.join(KEY_FILE);
        if std::fs::symlink_metadata(&path)
            .map_err(storage)?
            .file_type()
            .is_symlink()
        {
            return Err(format_error());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(storage)?
            .take(16_385)
            .read_to_end(&mut bytes)
            .map_err(storage)?;
        if bytes.len() > 16_384 {
            return Err(format_error());
        }
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| format_error())?;
        if record.v != 1 || record.kdf != KdfParams::DEFAULT {
            return Err(format_error());
        }
        Ok(record)
    }
    fn open(&self, pass: &str) -> Result<Key, AppError> {
        let record = self.read()?;
        let salt = B64.decode(record.salt).map_err(|_| format_error())?;
        if salt.len() != 16 {
            return Err(format_error());
        }
        let kek = crypto::derive_key(pass, &salt, &record.kdf).map_err(storage)?;
        let bytes = B64.decode(record.sealed_key).map_err(|_| format_error())?;
        let plain = Zeroizing::new(
            crypto::open(&kek, CONTEXT, &bytes).map_err(|_| AppError::WrongPassphrase)?,
        );
        Ok(Key::from_bytes(
            plain.as_slice().try_into().map_err(|_| format_error())?,
        ))
    }
    fn persist(&self, key: &Key, wrapping: &Wrapping, replace: bool) -> Result<(), AppError> {
        let record = Record {
            v: 1,
            salt: wrapping.salt.clone(),
            kdf: KdfParams::DEFAULT,
            sealed_key: B64
                .encode(crypto::seal(&wrapping.key, CONTEXT, key.as_bytes()).map_err(storage)?),
        };
        let bytes = serde_json::to_vec(&record).map_err(storage)?;
        std::fs::create_dir_all(&self.dir).map_err(storage)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))
                .map_err(storage)?;
        }
        let name = onepaste_core::id::new_id().map_err(storage)?;
        let temporary = self.dir.join(format!("{KEY_FILE}.{name}.tmp"));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(storage)?;
            file.write_all(&bytes).map_err(storage)?;
            file.sync_all().map_err(storage)?;
            drop(file);
            if replace {
                std::fs::rename(&temporary, self.dir.join(KEY_FILE)).map_err(storage)?;
            } else {
                // Create the destination atomically without replacing a concurrently created key record.
                std::fs::hard_link(&temporary, self.dir.join(KEY_FILE)).map_err(storage)?;
                std::fs::remove_file(&temporary).map_err(storage)?;
            }
            #[cfg(unix)]
            std::fs::File::open(&self.dir)
                .and_then(|f| f.sync_all())
                .map_err(storage)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

impl KeyStore for EncryptedKeyStore {
    fn ready_for_setup(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wrapping
            .is_some()
    }
    fn state(&self) -> Result<KeyState, AppError> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.key.is_some() {
            return Ok(KeyState::Unlocked);
        }
        if self.dir.join(KEY_FILE).try_exists().map_err(storage)? {
            return Ok(KeyState::Locked);
        }
        if has_existing_data(&self.dir)? && !(state.wrapping.is_some() && empty_setup(&self.dir)) {
            return Ok(KeyState::Recovery);
        }
        Ok(if state.wrapping.is_some() {
            KeyState::Prepared
        } else {
            KeyState::New
        })
    }
    fn prepare(&self, pass: &str) -> Result<(), AppError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if self.dir.join(KEY_FILE).try_exists().map_err(storage)? || has_existing_data(&self.dir)? {
            return Err(AppError::RecoveryRequired);
        }
        state.wrapping = Some(Self::wrapping(pass)?);
        Ok(())
    }
    fn save(&self, key: &Key) -> Result<(), AppError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if self.dir.join(KEY_FILE).try_exists().map_err(storage)? {
            return Err(AppError::AlreadySetUp);
        }
        let wrapping = state.wrapping.as_ref().ok_or(AppError::Locked)?;
        self.persist(key, wrapping, false)?;
        state.key = Some(key.clone());
        state.wrapping = None;
        Ok(())
    }
    fn load(&self) -> Result<Option<Key>, AppError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .key
            .clone())
    }
    fn unlock(&self, pass: &str) -> Result<(), AppError> {
        if pass.len() > 4096 {
            return Err(AppError::WrongPassphrase);
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let key = self.open(pass)?;
        state.key = Some(key);
        Ok(())
    }
    fn restore(&self, key: &Key, pass: &str) -> Result<(), AppError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.persist(key, &Self::wrapping(pass)?, true)?;
        state.key = Some(key.clone());
        state.wrapping = None;
        Ok(())
    }
    fn delete(&self) -> Result<(), AppError> {
        self.clear_session();
        match std::fs::remove_file(self.dir.join(KEY_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(storage(e)),
        }
    }
    fn clear_session(&self) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = State::default();
    }
}
