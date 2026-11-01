//! Explicit application-passphrase access and read-only recovery of existing application data.
use crate::{App, AppError, KeyState};
use onepaste_client::{db_path, SqliteStore};
use onepaste_core::{
    crypto::{self, aad, Key},
    keys::KeyParams,
    recovery,
};

impl App {
    pub fn key_state(&self) -> Result<KeyState, AppError> {
        if self.0.locked.load(std::sync::atomic::Ordering::Acquire) { return Ok(KeyState::Locked); }
        if self.is_joined() && self.expired() { return Ok(KeyState::Locked); }
        let state = self.0.secrets.state()?;
        if state == KeyState::New && crate::key_storage::has_existing_data(&self.0.dir)? {
            return Ok(KeyState::Recovery);
        }
        Ok(state)
    }
    pub fn prepare_local_passphrase(&self, pass: &str) -> Result<(), AppError> {
        if !matches!(self.key_state()?, KeyState::New | KeyState::Prepared) {
            return Err(AppError::AlreadySetUp);
        }
        self.0.secrets.prepare(pass)
    }
    pub async fn unlock_local(&self, pass: &str) -> Result<(), AppError> {
        if self.is_joined() {
            return Err(AppError::AlreadySetUp);
        }
        let _connection = self.0.connection.write().await;
        if self.is_joined() {
            return Err(AppError::AlreadySetUp);
        }
        self.0.secrets.unlock(pass)?;
        self.0.locked.store(false, std::sync::atomic::Ordering::Release);
        match self.start_session().await {
            Ok(true) => Ok(()),
            result => {
                self.0.locked.store(true, std::sync::atomic::Ordering::Release);
                self.0.secrets.clear_session();
                Err(result.err().unwrap_or(AppError::RecoveryRequired))
            }
        }
    }
    /// The supplied kit is checked against our existing encrypted files before the key record is changed.
    pub async fn recover_local(
        &self,
        credential: &str,
        is_kit: bool,
        new_pass: &str,
    ) -> Result<(), AppError> {
        if self.is_joined() {
            return Err(AppError::AlreadySetUp);
        }
        let _connection = self.0.connection.write().await;
        if self.is_joined() {
            return Err(AppError::AlreadySetUp);
        }
        let key = if is_kit {
            recovery::decode(credential).map_err(|_| AppError::WrongRecoveryKit)?
        } else {
            let store = SqliteStore::open_read_only(&db_path(&self.0.dir))
                .map_err(|_| AppError::WrongPassphrase)?;
            let bytes = store
                .meta_get("keyparams")?
                .ok_or(AppError::WrongPassphrase)?;
            if bytes.len() > 16_384 {
                return Err(AppError::WrongPassphrase);
            }
            let record: KeyParams =
                serde_json::from_str(&bytes).map_err(|_| AppError::WrongPassphrase)?;
            if record.kdf.m_kib > 256 * 1024 || record.kdf.t > 10 || record.kdf.p > 4 {
                return Err(AppError::WrongPassphrase);
            }
            record
                .unlock(credential)
                .map_err(|_| AppError::WrongPassphrase)?
        };
        self.validate_existing_key(&key).map_err(|_| {
            if is_kit {
                AppError::WrongRecoveryKit
            } else {
                AppError::WrongPassphrase
            }
        })?;
        self.0.secrets.restore(&key, new_pass)?;
        self.0.locked.store(false, std::sync::atomic::Ordering::Release);
        match self.start_session().await {
            Ok(true) => Ok(()),
            result => {
                self.0.locked.store(true, std::sync::atomic::Ordering::Release);
                self.0.secrets.clear_session();
                Err(result.err().unwrap_or(AppError::RecoveryRequired))
            }
        }
    }
    fn validate_existing_key(&self, key: &Key) -> Result<(), AppError> {
        let mut verified = false;
        if db_path(&self.0.dir)
            .try_exists()
            .map_err(|_| AppError::WrongKey)?
        {
            let store = SqliteStore::open_read_only(&db_path(&self.0.dir))?;
            if let Some(record) = store.meta_get("keyparams")? {
                let record: KeyParams =
                    serde_json::from_str(&record).map_err(|_| AppError::WrongKey)?;
                record.verify_mk(key).map_err(|_| AppError::WrongKey)?;
                let token = store.secret_get("devtoken")?.ok_or(AppError::WrongKey)?;
                crypto::open(key, aad::DEVTOKEN, &token).map_err(|_| AppError::WrongKey)?;
                verified = true;
            }
        }
        if self
            .0
            .dir
            .join(onepaste_history::FILE_NAME)
            .try_exists()
            .map_err(|_| AppError::WrongKey)?
        {
            onepaste_history::HistoryStore::verify_existing(&self.0.dir, key)
                .map_err(|_| AppError::WrongKey)?;
            verified = true;
        }
        if !verified {
            return Err(AppError::WrongKey);
        }
        Ok(())
    }
}
