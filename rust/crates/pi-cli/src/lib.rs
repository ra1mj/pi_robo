//! CLI composition boundary. No executable CLI is provided by the foundation milestone.

/// Marker proving the CLI crate participates in the workspace dependency graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CliBoundary;
