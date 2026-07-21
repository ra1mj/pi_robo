//! Composition contracts for a future runtime implementation.

use pi_protocol::AgentEvent;

/// Bounded event delivery boundary supplied by an embedding application.
pub trait EventSink: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn emit(&self, event: AgentEvent) -> Result<(), Self::Error>;
}
