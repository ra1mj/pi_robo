//! Resource discovery boundary. Loading behavior is outside this milestone.

/// Identifies a resource without selecting a storage implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceId(pub String);
