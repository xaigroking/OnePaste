//! Connection replacement helpers: safe errors and a monotonic history view across server sequences.
use crate::AppError;
use onepaste_client::{ClientError, SqliteStore};
use onepaste_core::crypto::Key;
use onepaste_history::{HistoryError, HistoryStore, ItemSource, SourceItems};

pub(crate) fn safe_error(error: ClientError) -> AppError {
    match error {
        ClientError::Server { code, .. } => AppError::Server(match code.as_str() {
            "registration_invalid" => "registration_invalid",
            "rate_limited" => "rate_limited",
            "unsupported_version" => "unsupported_version",
            _ => "server_change_failed",
        }.into()),
        ClientError::Storage(_) => AppError::Storage("server replacement".into()),
        ClientError::PassphrasePending => AppError::Server("passphrase_pending".into()),
        ClientError::WrongPassphrase => AppError::WrongPassphrase,
        ClientError::WrongRecoveryKit => AppError::WrongRecoveryKit,
        ClientError::WrongKey => AppError::WrongKey,
        ClientError::BadConnectionCode => AppError::InvalidConnectionCode,
        ClientError::PassphraseRequired => AppError::Server("passphrase_required".into()),
        ClientError::Transport(_) => AppError::Unreachable,
        _ => AppError::Server("server_change_failed".into()),
    }
}

struct HistoryView<'a>(&'a mut SqliteStore);

impl ItemSource for HistoryView<'_> {
    fn changed_since(&mut self, indexed: i64) -> Result<SourceItems, HistoryError> {
        let offset = self.0.meta_get("history_seq_offset").map_err(|_| HistoryError::Storage("history offset".into()))?
            .unwrap_or_else(|| "0".into()).parse::<i64>().map_err(|_| HistoryError::Storage("history offset".into()))?;
        let mut source = self.0.changed_since(indexed.saturating_sub(offset))?;
        for row in &mut source.rows {
            if row.seq > 0 { row.seq = row.seq.saturating_add(offset); }
        }
        Ok(source)
    }
}

pub(crate) fn refresh_history(history: &mut HistoryStore, store: &mut SqliteStore, key: &Key) -> Result<(), HistoryError> {
    history.refresh(&mut HistoryView(store), key).map(|_| ())
}
