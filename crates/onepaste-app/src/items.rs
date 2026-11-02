use super::*;

impl App {
    /// Searches the local history; an empty query lists the most recent items. Brings the history up to date first, so
    /// something copied a moment ago is found at once.
    pub async fn search(
        &self,
        query: &str,
        filter: Filter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ItemView>, AppError> {
        self.search_with_tag(query, filter, None, limit, offset)
            .await
    }

    pub async fn search_with_tag(
        &self,
        query: &str,
        filter: Filter,
        tag: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ItemView>, AppError> {
        let _connection = self.session_access().await?;
        if let Some(tag) = tag {
            onepaste_core::tags::key(tag).map_err(|_| AppError::InvalidTags)?;
        }
        self.refresh_history().await?;
        let hits = {
            let h = lock(&self.0.history);
            h.as_ref()
                .ok_or(AppError::NotJoined)?
                .search_with_tag(query, filter, tag, limit, offset)?
        };
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let (mut width, mut height) = (0, 0);
            if hit.kind == "image" {
                if let Ok(Ok(p)) = self.with_engine(|e| e.get_item(&hit.item_id)).await {
                    width = p.extra.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    height = p.extra.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
            }
            out.push(ItemView {
                item_id: hit.item_id,
                kind: hit.kind,
                preview: hit
                    .text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(PREVIEW_CHARS)
                    .collect(),
                favorite: hit.favorite,
                pinned: hit.pinned,
                note: hit.note,
                tags: hit.tags,
                language: hit.language,
                width,
                height,
                size: hit.size,
            });
        }
        Ok(out)
    }

    /// The picture of an image item. It is read from the history store and passed on: it is never logged, never put in the
    /// diagnostics and never written anywhere. A text item is `NotAnImage`.
    pub async fn item_image(&self, item_id: &str) -> Result<ImageData, AppError> {
        let _connection = self.session_access().await?;
        self.image_unlocked(item_id).await
    }

    pub(super) async fn image_unlocked(&self, item_id: &str) -> Result<ImageData, AppError> {
        let payload = self
            .with_engine(|e| e.get_item(item_id))
            .await?
            .map_err(|_| AppError::NotFound)?;
        if payload.kind != "image" {
            return Err(AppError::NotAnImage);
        }
        let data = payload.content.clone().ok_or(AppError::NotFound)?;
        let dim = |k: &str| payload.extra.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let size = payload
            .extra
            .get("size")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or_else(|| B64.decode(&data).map(|b| b.len()).unwrap_or(0));
        Ok(ImageData {
            data,
            width: dim("width"),
            height: dim("height"),
            size,
            format: "png".into(),
        })
    }

    /// The whole text of a text item (the list shows only its first line). Never logged. An image is `NotText`.
    pub async fn item_text(&self, item_id: &str) -> Result<String, AppError> {
        let _connection = self.session_access().await?;
        let payload = self
            .with_engine(|e| e.get_item(item_id))
            .await?
            .map_err(|_| AppError::NotFound)?;
        if payload.kind == "image" {
            return Err(AppError::NotText);
        }
        payload.content.ok_or(AppError::NotFound)
    }

    /// Puts an item back on the clipboard. The write carries the origin marker, so it does not become a new item.
    pub async fn copy(&self, item_id: &str) -> Result<(), AppError> {
        let _connection = self.session_access().await?;
        let payload = self
            .with_engine(|e| e.get_item(item_id))
            .await?
            .map_err(|_| AppError::NotFound)?;
        let content = content_of(&payload).ok_or(AppError::NotFound)?;
        self.0.clipboard.write(&content, true)
    }

    pub async fn set_flags(
        &self,
        item_id: &str,
        favorite: Option<bool>,
        pinned: Option<bool>,
        note: Option<String>,
    ) -> Result<(), AppError> {
        let _connection = self.session_access().await?;
        self.with_engine(|e| -> Result<(), onepaste_sync::Error> {
            if let Some(v) = favorite {
                e.enqueue_edit(now_ms(), item_id, Edit::Favorite(v))?;
            }
            if let Some(v) = pinned {
                e.enqueue_edit(now_ms(), item_id, Edit::Pinned(v))?;
            }
            if let Some(n) = note {
                e.enqueue_edit(
                    now_ms(),
                    item_id,
                    Edit::Note(if n.is_empty() { None } else { Some(n) }),
                )?;
            }
            Ok(())
        })
        .await?
        .map_err(AppError::from)?;
        self.0.wake.notify_one();
        Ok(())
    }
}
