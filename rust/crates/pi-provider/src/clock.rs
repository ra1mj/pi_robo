use std::time::{SystemTime, UNIX_EPOCH};

/// Millisecond clock injected into adapters for deterministic response timestamps.
pub trait ProviderClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Production wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProviderClock;

impl ProviderClock for SystemProviderClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}
