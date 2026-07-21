//! Session storage contracts without a filesystem or database implementation.

use pi_protocol::PersistedSessionRecord;
use std::future::Future;
use std::pin::Pin;

/// Storage error independent of a concrete backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreError {
    pub message: String,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

/// Append-only session persistence boundary.
pub trait SessionStore: Send + Sync {
    fn load<'a>(&'a self, session_id: &'a str) -> StoreFuture<'a, Vec<PersistedSessionRecord>>;
    fn append<'a>(
        &'a self,
        session_id: &'a str,
        record: &'a PersistedSessionRecord,
    ) -> StoreFuture<'a, ()>;
}
