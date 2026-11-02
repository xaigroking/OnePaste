use super::*;

impl App {
    /// Corrects only private classification metadata; the original text remains immutable.
    pub async fn set_text_classification(
        &self,
        item_id: &str,
        kind: &str,
        language: Option<&str>,
    ) -> Result<(), AppError> {
        let _connection = self.session_access().await?;
        self.with_engine(|engine| -> Result<(), AppError> {
            let payload = engine.get_item(item_id).map_err(|_| AppError::NotFound)?;
            if payload.kind != "text" {
                return Err(AppError::NotText);
            }
            let value = onepaste_core::classification::Classification::corrected(
                payload.content.as_deref().unwrap_or(""),
                kind,
                language,
            )
            .map_err(|_| AppError::InvalidClassification)?;
            engine
                .enqueue_edit(now_ms(), item_id, Edit::TextClassification(value))
                .map_err(AppError::from)?;
            Ok(())
        })
        .await??;
        self.0.wake.notify_one();
        Ok(())
    }
}
