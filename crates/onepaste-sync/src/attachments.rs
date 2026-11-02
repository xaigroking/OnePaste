//! Attachment preconditions use the same durable item intents without blocking unrelated queue entries.
use crate::engine::{Engine, Event, InFlight};
use crate::intent::{open_envelope, parse_envelope};
use crate::{Error, OpState, Store};
use onepaste_core::attachment::Attachment;

impl<S: Store> Engine<S> {
    pub fn attachment(&mut self, id: &str) -> Result<Attachment, Error> {
        let key = &self.mk;
        let mut result = None;
        self.store.transact(&mut |tx| {
            result = tx.item(id)?;
            Ok(())
        })?;
        let row = result.ok_or(Error::NotFound)?;
        if row.deleted {
            return Err(Error::Deleted);
        }
        let envelope = parse_envelope(row.envelope_json.as_deref().ok_or(Error::NotFound)?)?;
        if envelope.id != id {
            return Err(Error::MalformedResponse);
        }
        let descriptor = Attachment::from_payload(&open_envelope(key, &envelope)?)?;
        if envelope.blob_id.as_deref() != Some(descriptor.blob_id.as_str()) {
            return Err(Error::MalformedResponse);
        }
        Ok(descriptor)
    }

    pub fn begin_attachment_preparation(&mut self) -> Result<(), Error> {
        match self.inflight {
            Some(InFlight::Push { op_id }) => {
                self.inflight = Some(InFlight::AttachmentPreparation { op_id });
                Ok(())
            }
            _ => Err(Error::MalformedResponse),
        }
    }

    pub fn finish_attachment_preparation(&mut self) -> Result<bool, Error> {
        let Some(InFlight::AttachmentPreparation { op_id }) = self.inflight else {
            return Err(Error::MalformedResponse);
        };
        let pending = self.read(|tx| {
            Ok(tx
                .outbox_row(op_id)?
                .is_some_and(|op| op.state == OpState::Pending))
        })?;
        self.inflight = if pending {
            Some(InFlight::Push { op_id })
        } else {
            None
        };
        Ok(pending)
    }

    /// Queued intents also retain chunks when a delete follows an already-dispatched create.
    pub fn attachment_references(&mut self) -> Result<std::collections::HashSet<String>, Error> {
        let key = &self.mk;
        let mut envelopes = Vec::new();
        self.store.transact(&mut |tx| {
            for row in tx.items()?.into_iter().filter(|row| !row.deleted) {
                if let Some(json) = row.envelope_json {
                    envelopes.push(Ok(json));
                }
            }
            for operation in tx.outbox()? {
                envelopes.push(
                    crate::intent::open_blob(key, operation.op_id, &operation.sealed_intent)
                        .map(|blob| blob.envelope.unwrap_or_default()),
                );
            }
            Ok(())
        })?;
        let mut references = std::collections::HashSet::new();
        for envelope in envelopes {
            let envelope = envelope?;
            if envelope.is_empty() {
                continue;
            }
            if let Some(id) = parse_envelope(&envelope)?.blob_id {
                references.insert(id);
            }
        }
        Ok(references)
    }

    pub fn fail_attachment_push(&mut self, code: &'static str) -> Result<Vec<Event>, Error> {
        let op_id = match self.inflight.take() {
            Some(InFlight::Push { op_id } | InFlight::AttachmentPreparation { op_id }) => op_id,
            other => {
                self.inflight = other;
                return Err(Error::MalformedResponse);
            }
        };
        let Some(item_id) = self.read(|tx| Ok(tx.outbox_row(op_id)?.map(|op| op.item_id)))? else {
            return Ok(Vec::new());
        };
        self.fail_op(op_id, code)?;
        self.push_retry_at = 0;
        Ok(vec![Event::OpFailed {
            op_id,
            item_id,
            code: code.into(),
        }])
    }

    pub fn retry_attachment(&mut self, id: &str) -> Result<usize, Error> {
        self.attachment(id)?;
        let mut count = 0;
        self.read(|tx| {
            for mut operation in tx.outbox()?.into_iter().filter(|op| {
                op.item_id == id
                    && op.state == OpState::Failed
                    && op
                        .last_error
                        .as_deref()
                        .is_some_and(|code| code.starts_with("attachment_"))
            }) {
                operation.state = OpState::Pending;
                operation.last_error = None;
                tx.put_outbox(&operation)?;
                count += 1;
            }
            Ok(())
        })?;
        self.push_retry_at = 0;
        Ok(count)
    }
}
