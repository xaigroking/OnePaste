//! Actual file bytes stay on the device until encrypted; plaintext exports require an explicit caller path.
use super::*;
use onepaste_core::attachment::{chunk_count, CHUNK_BYTES, MAX_FILE_BYTES, VERSION};
use std::io::{Read, Write};
use std::path::Path;
use zeroize::Zeroizing;

fn mime(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}
fn verified(
    root: &Path,
    descriptor: &Attachment,
    item_id: &str,
    key: &Key,
) -> Result<Vec<u8>, AppError> {
    let mut output = Zeroizing::new(Vec::with_capacity(descriptor.size as usize));
    for index in 0..descriptor.chunks {
        let sealed = cache::read(root, descriptor, index)?;
        let plain = Zeroizing::new(
            descriptor
                .open_chunk(key, item_id, index, &sealed)
                .map_err(|_| AppError::Attachment("attachment_integrity"))?,
        );
        output.extend_from_slice(&plain);
    }
    descriptor
        .verify(&output)
        .map_err(|_| AppError::Attachment("attachment_integrity"))?;
    Ok(std::mem::take(&mut *output))
}

impl App {
    pub async fn import_file(&self, path: &Path) -> Result<String, AppError> {
        self.import_file_at(path, self.session_epoch()).await
    }
    pub async fn import_file_at(&self, path: &Path, epoch: u64) -> Result<String, AppError> {
        let _access = self.session_access().await?;
        self.attachment_epoch(epoch)?;
        let _io = self.0.attachment_io.lock().await;
        let mut file = std::fs::File::open(path)
            .map_err(|_| AppError::Attachment("attachment_read_failed"))?;
        let before = file
            .metadata()
            .map_err(|_| AppError::Attachment("attachment_read_failed"))?;
        if !before.is_file() {
            return Err(AppError::Attachment("attachment_read_failed"));
        }
        if before.len() > MAX_FILE_BYTES {
            return Err(AppError::Attachment("attachment_too_large"));
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AppError::Attachment("attachment_metadata"))?
            .to_owned();
        let item_id =
            onepaste_core::id::new_id().map_err(|_| AppError::Attachment("attachment_storage"))?;
        let blob_id =
            onepaste_core::id::new_id().map_err(|_| AppError::Attachment("attachment_storage"))?;
        let mut descriptor = Attachment {
            v: VERSION,
            blob_id: blob_id.clone(),
            mime: mime(&file_name).into(),
            file_name,
            size: before.len(),
            sha256: "0".repeat(64),
            chunk_bytes: CHUNK_BYTES as u32,
            chunks: chunk_count(before.len()),
        };
        descriptor
            .validate()
            .map_err(|_| AppError::Attachment("attachment_metadata"))?;
        let key = self
            .with_engine(|engine| engine.master_key().clone())
            .await?;
        cache::create(&self.0.dir, &blob_id)?;
        let result = async {
            let mut hash = Sha256::new();
            for index in 0..descriptor.chunks {
                self.attachment_epoch(epoch)?;
                let mut plain = Zeroizing::new(vec![
                    0;
                    descriptor.plain_chunk_len(index).map_err(
                        |_| AppError::Attachment("attachment_metadata")
                    )?
                ]);
                file.read_exact(&mut plain)
                    .map_err(|_| AppError::Attachment("attachment_changed"))?;
                hash.update(&*plain);
                let sealed = descriptor
                    .seal_chunk(&key, &item_id, index, &plain)
                    .map_err(|_| AppError::Attachment("attachment_integrity"))?;
                cache::write(&self.0.dir, &descriptor, index, &sealed)?;
                tokio::task::yield_now().await;
            }
            let mut probe = [0];
            if file
                .read(&mut probe)
                .map_err(|_| AppError::Attachment("attachment_read_failed"))?
                != 0
                || before.modified().ok()
                    != file.metadata().ok().and_then(|meta| meta.modified().ok())
            {
                return Err(AppError::Attachment("attachment_changed"));
            }
            descriptor.sha256 = hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let mut payload = descriptor
                .payload()
                .map_err(|_| AppError::Attachment("attachment_metadata"))?;
            payload.device =
                Some(lock(&self.0.shared).device_id.clone()).filter(|name| !name.is_empty());
            self.attachment_epoch(epoch)?;
            self.with_engine(|engine| engine.enqueue_attachment(now_ms(), &item_id, &payload))
                .await?
                .map_err(AppError::from)
        }
        .await;
        if result.is_err() {
            cache::remove(&self.0.dir, &blob_id);
        } else {
            self.0.wake.notify_one();
            self.notify_status();
        }
        result
    }

    pub async fn attachment_bytes(&self, id: &str) -> Result<Vec<u8>, AppError> {
        let epoch = self.session_epoch();
        let (descriptor, api, serial, mut cancel) = {
            let _access = self.session_access().await?;
            self.attachment_epoch(epoch)?;
            let descriptor = self
                .with_engine(|engine| engine.attachment(id))
                .await?
                .map_err(metadata)?;
            if cache::complete(&self.0.dir, &descriptor) {
                let key = self
                    .with_engine(|engine| engine.master_key().clone())
                    .await?;
                let result = verified(&self.0.dir, &descriptor, id, &key);
                self.attachment_epoch(epoch)?;
                if result.is_ok() || self.mode() == Mode::LocalOnly {
                    return result;
                }
            }
            if self.mode() == Mode::LocalOnly {
                return Err(AppError::Attachment("attachment_unavailable"));
            }
            let api = {
                let session = self.0.session.lock().await;
                session.as_ref().ok_or(AppError::NotJoined)?.api()
            };
            let (serial, cancel) = self.begin_transfer(id, "download", descriptor.chunks)?;
            (descriptor, api, serial, cancel)
        };
        let result = async {
            capable(&api, &mut cancel).await?;
            for index in 0..descriptor.chunks {
                let valid = {
                    let _access = self.session_access().await?;
                    self.attachment_epoch(epoch)?;
                    let key = self
                        .with_engine(|engine| engine.master_key().clone())
                        .await?;
                    cache::read(&self.0.dir, &descriptor, index)
                        .ok()
                        .and_then(|sealed| descriptor.open_chunk(&key, id, index, &sealed).ok())
                        .map(Zeroizing::new)
                        .is_some()
                };
                if !valid {
                    let r = response(
                        &mut cancel,
                        api.download_attachment_chunk(&descriptor.blob_id, index),
                    )
                    .await?;
                    accepted(&r)?;
                    if r.header("onepaste-blob-version") != Some("1")
                        || r.header("onepaste-blob-chunks")
                            .and_then(|value| value.parse::<u32>().ok())
                            != Some(descriptor.chunks)
                    {
                        return Err(AppError::Attachment("attachment_integrity"));
                    }
                    let _access = self.session_access().await?;
                    self.attachment_epoch(epoch)?;
                    let key = self
                        .with_engine(|engine| engine.master_key().clone())
                        .await?;
                    let _plain = Zeroizing::new(
                        descriptor
                            .open_chunk(&key, id, index, &r.body)
                            .map_err(|_| AppError::Attachment("attachment_integrity"))?,
                    );
                    let _io = self.0.attachment_io.lock().await;
                    cache::write(&self.0.dir, &descriptor, index, &r.body)?;
                }
                self.progress(id, serial, index + 1);
            }
            let _access = self.session_access().await?;
            self.attachment_epoch(epoch)?;
            let key = self
                .with_engine(|engine| engine.master_key().clone())
                .await?;
            let bytes = verified(&self.0.dir, &descriptor, id, &key)?;
            self.attachment_epoch(epoch)?;
            Ok(bytes)
        }
        .await;
        let status = result.as_ref().map(|_| ()).map_err(Clone::clone);
        self.finish_transfer(id, serial, &status);
        result
    }

    pub async fn prepare_attachment(&self, id: &str) -> Result<(), AppError> {
        let bytes = Zeroizing::new(self.attachment_bytes(id).await?);
        drop(bytes);
        Ok(())
    }

    pub async fn save_attachment_at(
        &self,
        id: &str,
        path: &Path,
        epoch: u64,
    ) -> Result<(), AppError> {
        if epoch != self.session_epoch() {
            return Err(AppError::Locked);
        }
        let bytes = Zeroizing::new(self.attachment_bytes(id).await?);
        let _access = self.session_access().await?;
        self.attachment_epoch(epoch)?;
        let parent = path
            .parent()
            .ok_or(AppError::Attachment("attachment_write_failed"))?;
        let temporary = parent.join(format!(
            ".onepaste-export-{}.tmp",
            onepaste_core::id::new_id()
                .map_err(|_| AppError::Attachment("attachment_write_failed"))?
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|_| AppError::Attachment("attachment_write_failed"))?;
            file.write_all(&bytes)
                .map_err(|_| AppError::Attachment("attachment_write_failed"))?;
            file.sync_all()
                .map_err(|_| AppError::Attachment("attachment_write_failed"))?;
            drop(file);
            self.attachment_epoch(epoch)?;
            std::fs::rename(&temporary, path)
                .map_err(|_| AppError::Attachment("attachment_write_failed"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}
