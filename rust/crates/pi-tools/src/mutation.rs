use crate::path::mutation_key;
use pi_agent::{Cancellation, ToolError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Clone, Debug, Default)]
pub struct MutationCoordinator {
    entries: Arc<Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>>,
}

impl MutationCoordinator {
    pub async fn acquire(
        &self,
        path: &Path,
        cancellation: &dyn Cancellation,
    ) -> Result<MutationLease, ToolError> {
        if cancellation.is_cancelled() {
            return Err(ToolError::cancelled());
        }
        let key = mutation_key(path).await?;
        let lock = {
            let mut entries = self.entries.lock().await;
            Arc::clone(
                entries
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let guard = tokio::select! {
            guard = lock.lock_owned() => guard,
            () = cancellation.cancelled() => return Err(ToolError::cancelled()),
        };
        Ok(MutationLease { key, _guard: guard })
    }
}

#[derive(Debug)]
pub struct MutationLease {
    key: PathBuf,
    _guard: OwnedMutexGuard<()>,
}

impl MutationLease {
    #[must_use]
    pub fn key(&self) -> &Path {
        &self.key
    }
}
