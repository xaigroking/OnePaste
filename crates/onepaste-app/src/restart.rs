//! An explicit fresh start does not require access to the previous encryption key.
use super::*;

impl App {
    /// Sets aside this computer's old encrypted files after the user chooses to start fresh.
    /// No old password is required and no server or other device is contacted.
    pub async fn restart_local_setup(&self) -> Result<(), AppError> {
        let _connection = self.0.connection.write().await;
        self.close_session().await;
        archive_local_files(&self.0.dir)?;
        self.0.secrets.delete()?;
        {
            let mut shared = lock(&self.0.shared);
            shared.mode = Mode::Unset;
            shared.device_id.clear();
            shared.paused = None;
            shared.incompatible = false;
            shared.initial_sync_done = false;
            shared.tray = TrayState::Synced;
            shared.log.clear();
        }
        self.0
            .locked
            .store(false, std::sync::atomic::Ordering::Release);
        self.log("local_setup_restarted", None);
        self.notify_status();
        Ok(())
    }
}

fn archive_local_files(dir: &std::path::Path) -> Result<(), AppError> {
    let storage = |e: std::io::Error| AppError::Storage(e.to_string());
    let mut names = Vec::new();
    // Keep application preferences, the instance lock and unrelated files in place.
    for base in [DB_FILE, "history.db", STATE_FILE_NAME, crate::KEY_FILE] {
        for suffix in ["", "-wal", "-shm", "-journal", ".tmp"] {
            let name = format!("{base}{suffix}");
            match std::fs::symlink_metadata(dir.join(&name)) {
                Ok(_) => names.push(name),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(storage(e)),
            }
        }
    }
    if names.is_empty() {
        return Ok(());
    }
    let archive = dir.join(format!(
        "previous-data-{}",
        onepaste_core::id::new_id().map_err(|e| AppError::Other(e.code().into()))?
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&archive).map_err(storage)?;
    let mut moved = Vec::new();
    for name in names {
        if let Err(e) = std::fs::rename(dir.join(&name), archive.join(&name)) {
            // Restore every completed move if a file cannot be set aside. Never delete the archive on rollback failure.
            let mut failures = Vec::new();
            for previous in moved.iter().rev() {
                if let Err(rollback) = std::fs::rename(archive.join(previous), dir.join(previous)) {
                    failures.push(rollback.to_string());
                }
            }
            if failures.is_empty() {
                if let Err(cleanup) = std::fs::remove_dir(&archive) {
                    failures.push(cleanup.to_string());
                }
            }
            return Err(AppError::Storage(format!(
                "{e}; rollback errors: {}",
                failures.join("; ")
            )));
        }
        moved.push(name);
    }
    Ok(())
}
