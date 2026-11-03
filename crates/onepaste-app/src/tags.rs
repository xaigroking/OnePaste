use super::*;

impl App {
    pub async fn set_tags(&self, item_id: &str, tags: &[String]) -> Result<Vec<String>, AppError> {
        let _connection = self.session_access().await?;
        let tags = onepaste_core::tags::normalize(tags).map_err(|_| AppError::InvalidTags)?;
        self.with_engine(|engine| engine.enqueue_edit(now_ms(), item_id, Edit::Tags(tags.clone())))
            .await?
            .map_err(AppError::from)?;
        self.0.wake.notify_one();
        Ok(tags)
    }

    pub async fn tag_options(
        &self,
        query: &str,
    ) -> Result<Vec<onepaste_history::TagOption>, AppError> {
        let _connection = self.session_access().await?;
        onepaste_core::tags::key(query).map_err(|_| AppError::InvalidTags)?;
        self.refresh_history().await?;
        lock(&self.0.history)
            .as_ref()
            .ok_or(AppError::NotJoined)?
            .tag_options(query)
            .map_err(AppError::from)
    }
}
