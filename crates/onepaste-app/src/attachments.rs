//! Private file metadata and cancellable attachment transfers.
use super::*;
use onepaste_client::{ApiClient, ClientError};
use onepaste_core::attachment::Attachment;
use onepaste_sync::HttpResponse;
use tokio::sync::watch;

#[path = "attachment_cache.rs"]
mod cache;
#[path = "attachment_io.rs"]
mod file_io;
pub(super) use cache::DIRECTORY;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub state: String,
    pub direction: String,
    pub completed: u32,
    pub total: u32,
    pub error: Option<String>,
}
impl Default for TransferProgress {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            direction: "download".into(),
            completed: 0,
            total: 0,
            error: None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub file_name: String,
    pub mime: String,
    pub size: u64,
    pub cached: bool,
    pub sync_state: String,
    pub transfer: TransferProgress,
}
pub(super) struct TransferEntry {
    serial: u64,
    active: bool,
    cancel: watch::Sender<bool>,
    progress: TransferProgress,
}

fn metadata(error: onepaste_sync::Error) -> AppError {
    match error {
        onepaste_sync::Error::NotFound | onepaste_sync::Error::Deleted => AppError::NotFound,
        onepaste_sync::Error::Core(onepaste_core::Error::UnsupportedVersion(_)) => {
            AppError::Attachment("attachment_unsupported")
        }
        onepaste_sync::Error::Core(onepaste_core::Error::TooLarge) => {
            AppError::Attachment("attachment_too_large")
        }
        _ => AppError::Attachment("attachment_metadata"),
    }
}
fn failure_code(error: &AppError) -> &'static str {
    match error {
        AppError::Attachment(code) => code,
        AppError::Locked => "attachment_cancelled",
        _ => "attachment_transfer_failed",
    }
}
async fn response(
    cancel: &mut watch::Receiver<bool>,
    future: impl std::future::Future<Output = Result<HttpResponse, ClientError>>,
) -> Result<HttpResponse, AppError> {
    if *cancel.borrow() {
        return Err(AppError::Attachment("attachment_cancelled"));
    }
    tokio::select! {biased; _=cancel.wait_for(|value|*value)=>Err(AppError::Attachment("attachment_cancelled")),result=future=>result.map_err(|_|AppError::Attachment("attachment_transfer_failed"))}
}
fn accepted(response: &HttpResponse) -> Result<(), AppError> {
    match response.status {
        200..=299 => Ok(()),
        401 => Err(AppError::Revoked),
        404 => Err(AppError::Attachment("attachment_unavailable")),
        _ => Err(AppError::Attachment("attachment_transfer_failed")),
    }
}
async fn capable(api: &ApiClient, cancel: &mut watch::Receiver<bool>) -> Result<(), AppError> {
    let r = response(cancel, api.attachment_capabilities()).await?;
    if r.status == 404 {
        return Err(AppError::Attachment("attachment_unsupported"));
    }
    accepted(&r)?;
    let value: serde_json::Value =
        serde_json::from_slice(&r.body).map_err(|_| AppError::Attachment("attachment_metadata"))?;
    if !value["features"].as_array().is_some_and(|features| {
        features
            .iter()
            .any(|feature| feature.as_str() == Some("attachments-v1"))
    }) {
        return Err(AppError::Attachment("attachment_unsupported"));
    }
    Ok(())
}

impl App {
    fn begin_transfer(
        &self,
        id: &str,
        direction: &str,
        total: u32,
    ) -> Result<(u64, watch::Receiver<bool>), AppError> {
        let mut entries = lock(&self.0.attachment_transfers);
        if entries.get(id).is_some_and(|entry| entry.active) {
            return Err(AppError::Attachment("attachment_busy"));
        }
        let serial = self
            .0
            .attachment_sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (cancel, receiver) = watch::channel(false);
        entries.insert(
            id.into(),
            TransferEntry {
                serial,
                active: true,
                cancel,
                progress: TransferProgress {
                    state: if direction == "upload" {
                        "uploading"
                    } else {
                        "downloading"
                    }
                    .into(),
                    direction: direction.into(),
                    completed: 0,
                    total,
                    error: None,
                },
            },
        );
        drop(entries);
        self.notify_status();
        Ok((serial, receiver))
    }
    fn progress(&self, id: &str, serial: u64, completed: u32) {
        if let Some(entry) = lock(&self.0.attachment_transfers)
            .get_mut(id)
            .filter(|entry| entry.serial == serial)
        {
            entry.progress.completed = completed;
        }
        if completed % 8 == 0 {
            self.notify_status();
        }
    }
    fn finish_transfer(&self, id: &str, serial: u64, result: &Result<(), AppError>) {
        if let Some(entry) = lock(&self.0.attachment_transfers)
            .get_mut(id)
            .filter(|entry| entry.serial == serial)
        {
            entry.active = false;
            entry.progress.state = if result.is_ok() {
                "ready"
            } else if matches!(
                result,
                Err(AppError::Locked | AppError::Attachment("attachment_cancelled"))
            ) {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            entry.progress.error = result
                .as_ref()
                .err()
                .map(|error| failure_code(error).into());
        }
        self.notify_status();
    }
    pub fn cancel_attachment(&self, id: &str) {
        if let Some(entry) = lock(&self.0.attachment_transfers).get(id) {
            entry.cancel.send_replace(true);
        }
    }
    pub(super) fn cancel_attachment_transfers(&self) {
        for entry in lock(&self.0.attachment_transfers).values() {
            entry.cancel.send_replace(true);
        }
    }
    fn attachment_epoch(&self, epoch: u64) -> Result<(), AppError> {
        self.require_access()?;
        if self.session_epoch() != epoch {
            return Err(AppError::Locked);
        }
        Ok(())
    }

    pub async fn attachment_info(&self, id: &str) -> Result<AttachmentInfo, AppError> {
        let _access = self.session_access().await?;
        let desc = self
            .with_engine(|engine| engine.attachment(id))
            .await?
            .map_err(metadata)?;
        let mut transfer = lock(&self.0.attachment_transfers)
            .get(id)
            .map(|entry| entry.progress.clone())
            .unwrap_or(TransferProgress {
                total: desc.chunks,
                ..Default::default()
            });
        let operations = self
            .with_engine(|engine| engine.outbox())
            .await?
            .map_err(AppError::from)?;
        let pending: Vec<_> = operations
            .iter()
            .filter(|operation| operation.item_id == id)
            .collect();
        let sync_state = if self.mode() == Mode::LocalOnly {
            "local"
        } else if pending
            .iter()
            .any(|operation| operation.state == onepaste_sync::OpState::Failed)
        {
            "failed"
        } else if !pending.is_empty() {
            "pending"
        } else {
            "synced"
        };
        if transfer.state == "idle" && sync_state == "failed" {
            transfer.state = "failed".into();
            transfer.direction = "upload".into();
            transfer.error = Some("attachment_transfer_failed".into());
        }
        Ok(AttachmentInfo {
            file_name: desc.file_name.clone(),
            mime: desc.mime.clone(),
            size: desc.size,
            cached: cache::complete(&self.0.dir, &desc),
            sync_state: sync_state.into(),
            transfer,
        })
    }

    pub async fn retry_attachment(&self, id: &str) -> Result<(), AppError> {
        let _access = self.session_access().await?;
        self.with_engine(|engine| engine.retry_attachment(id))
            .await?
            .map_err(metadata)?;
        lock(&self.0.attachment_transfers).remove(id);
        self.0.wake.notify_one();
        self.notify_status();
        Ok(())
    }

    pub(super) async fn upload_attachment(&self, id: &str) -> Result<(), AppError> {
        let epoch = self.session_epoch();
        self.attachment_epoch(epoch)?;
        let descriptor = self
            .with_engine(|engine| engine.attachment(id))
            .await?
            .map_err(metadata)?;
        let api = {
            let session = self.0.session.lock().await;
            session.as_ref().ok_or(AppError::NotJoined)?.api()
        };
        let (serial, mut cancel) = self.begin_transfer(id, "upload", descriptor.chunks)?;
        let result = async {
            capable(&api, &mut cancel).await?;
            let manifest =
                response(&mut cancel, api.attachment_manifest(&descriptor.blob_id)).await?;
            let mut present = HashSet::new();
            if manifest.status != 404 {
                accepted(&manifest)?;
                let value: serde_json::Value = serde_json::from_slice(&manifest.body)
                    .map_err(|_| AppError::Attachment("attachment_metadata"))?;
                if value["v"].as_u64() != Some(1)
                    || value["chunks"].as_u64() != Some(u64::from(descriptor.chunks))
                {
                    return Err(AppError::Attachment("attachment_integrity"));
                }
                let indices = value["indices"]
                    .as_array()
                    .ok_or(AppError::Attachment("attachment_metadata"))?;
                for index in indices {
                    let index = index
                        .as_u64()
                        .filter(|index| *index < u64::from(descriptor.chunks))
                        .ok_or(AppError::Attachment("attachment_metadata"))?;
                    present.insert(index as u32);
                }
            }
            self.progress(id, serial, present.len() as u32);
            for index in 0..descriptor.chunks {
                self.attachment_epoch(epoch)?;
                if present.contains(&index) {
                    continue;
                }
                let sealed = cache::read(&self.0.dir, &descriptor, index)?;
                {
                    let plain = self
                        .with_engine(|engine| {
                            descriptor.open_chunk(engine.master_key(), id, index, &sealed)
                        })
                        .await?
                        .map_err(|_| AppError::Attachment("attachment_integrity"))?;
                    drop(zeroize::Zeroizing::new(plain));
                }
                let r = response(
                    &mut cancel,
                    api.upload_attachment_chunk(
                        &descriptor.blob_id,
                        index,
                        descriptor.chunks,
                        sealed,
                    ),
                )
                .await?;
                accepted(&r)?;
                self.attachment_epoch(epoch)?;
                present.insert(index);
                self.progress(id, serial, present.len() as u32);
            }
            Ok(())
        }
        .await;
        self.finish_transfer(id, serial, &result);
        result
    }

    pub(super) async fn cache_attachments_for_server_change(&self) -> Result<u64, AppError> {
        let epoch = self.session_epoch();
        let ids = {
            let _access = self.session_access().await?;
            self.with_engine(|engine| engine.list_items())
                .await?
                .map_err(AppError::from)?
                .into_iter()
                .filter(|item| !item.deleted && item.kind == "file")
                .map(|item| item.id)
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.prepare_attachment(&id).await?;
        }
        self.attachment_epoch(epoch)?;
        Ok(epoch)
    }

    pub(super) fn attachment_cached(&self, descriptor: &Attachment) -> bool {
        cache::complete(&self.0.dir, descriptor)
    }

    pub(super) async fn prune_attachment_cache(&self) {
        let _io = self.0.attachment_io.lock().await;
        if let Ok(Ok(references)) = self
            .with_engine(|engine| engine.attachment_references())
            .await
        {
            let _ = cache::prune(&self.0.dir, &references);
        }
    }
}
