//! The storage contract. The engine never touches a database; each platform implements `Store`.

use crate::types::{ItemRow, OutboxRow, SyncMeta};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum StoreError {
    /// The engine decided to abandon the transaction. Everything done inside it is rolled back.
    #[error("transaction aborted")]
    Aborted,
    #[error("storage failure: {0}")]
    Failed(String),
}

/// Operations available inside one transaction.
pub trait Tx {
    fn meta(&self) -> Result<SyncMeta, StoreError>;
    fn set_meta(&mut self, meta: &SyncMeta) -> Result<(), StoreError>;
    fn item(&self, id: &str) -> Result<Option<ItemRow>, StoreError>;
    fn put_item(&mut self, row: &ItemRow) -> Result<(), StoreError>;
    fn remove_item(&mut self, id: &str) -> Result<(), StoreError>;
    /// All items, ordered by id.
    fn items(&self) -> Result<Vec<ItemRow>, StoreError>;
    fn clear_items(&mut self) -> Result<(), StoreError>;
    fn outbox_row(&self, op_id: u64) -> Result<Option<OutboxRow>, StoreError>;
    fn put_outbox(&mut self, row: &OutboxRow) -> Result<(), StoreError>;
    fn remove_outbox(&mut self, op_id: u64) -> Result<(), StoreError>;
    /// All queued operations, ascending by `op_id`.
    fn outbox(&self) -> Result<Vec<OutboxRow>, StoreError>;
    /// A named point right before the transaction commits. The engine does nothing here; a platform store may use it
    /// for diagnostics or test instrumentation.
    fn checkpoint(&mut self, _name: &'static str) {}
}

pub trait Store {
    /// Runs `f` as one transaction: if `f` or the commit fails, **no change is left behind**.
    /// `f` is called exactly once.
    fn transact(&mut self, f: &mut dyn FnMut(&mut dyn Tx) -> Result<(), StoreError>) -> Result<(), StoreError>;
}

#[derive(Clone, Debug, Default)]
struct MemState {
    meta: SyncMeta,
    items: BTreeMap<String, ItemRow>,
    outbox: BTreeMap<u64, OutboxRow>,
}

/// An in-memory store with snapshot rollback, for tests and for platforms that bring their own persistence later.
#[derive(Clone, Debug, Default)]
pub struct MemStore {
    state: MemState,
    /// Test hook: the transaction fails when it tries its Nth write (0 means the first write).
    pub fail_at_write: Option<usize>,
}

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }
}

struct MemTx {
    state: MemState,
    writes: usize,
    fail_at_write: Option<usize>,
}

impl MemTx {
    fn write(&mut self) -> Result<(), StoreError> {
        let n = self.writes;
        self.writes += 1;
        if self.fail_at_write == Some(n) {
            return Err(StoreError::Failed("injected failure".into()));
        }
        Ok(())
    }
}

impl Tx for MemTx {
    fn meta(&self) -> Result<SyncMeta, StoreError> {
        Ok(self.state.meta.clone())
    }

    fn set_meta(&mut self, meta: &SyncMeta) -> Result<(), StoreError> {
        self.write()?;
        self.state.meta = meta.clone();
        Ok(())
    }

    fn item(&self, id: &str) -> Result<Option<ItemRow>, StoreError> {
        Ok(self.state.items.get(id).cloned())
    }

    fn put_item(&mut self, row: &ItemRow) -> Result<(), StoreError> {
        self.write()?;
        self.state.items.insert(row.id.clone(), row.clone());
        Ok(())
    }

    fn remove_item(&mut self, id: &str) -> Result<(), StoreError> {
        self.write()?;
        self.state.items.remove(id);
        Ok(())
    }

    fn items(&self) -> Result<Vec<ItemRow>, StoreError> {
        Ok(self.state.items.values().cloned().collect())
    }

    fn clear_items(&mut self) -> Result<(), StoreError> {
        self.write()?;
        self.state.items.clear();
        Ok(())
    }

    fn outbox_row(&self, op_id: u64) -> Result<Option<OutboxRow>, StoreError> {
        Ok(self.state.outbox.get(&op_id).cloned())
    }

    fn put_outbox(&mut self, row: &OutboxRow) -> Result<(), StoreError> {
        self.write()?;
        self.state.outbox.insert(row.op_id, row.clone());
        Ok(())
    }

    fn remove_outbox(&mut self, op_id: u64) -> Result<(), StoreError> {
        self.write()?;
        self.state.outbox.remove(&op_id);
        Ok(())
    }

    fn outbox(&self) -> Result<Vec<OutboxRow>, StoreError> {
        Ok(self.state.outbox.values().cloned().collect())
    }
}

impl Store for MemStore {
    fn transact(&mut self, f: &mut dyn FnMut(&mut dyn Tx) -> Result<(), StoreError>) -> Result<(), StoreError> {
        let mut tx = MemTx { state: self.state.clone(), writes: 0, fail_at_write: self.fail_at_write };
        f(&mut tx)?;
        self.state = tx.state;
        Ok(())
    }
}
