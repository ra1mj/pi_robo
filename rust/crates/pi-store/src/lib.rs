//! Read-only configuration stores and append-only session-v3 persistence.

mod auth;
mod error;
mod models;
mod paths;
mod session;
mod settings;
mod trust;

pub use auth::{
    AuthDocument, AuthRecord, CancellationFuture, CommandCancellation, CommandRequest,
    CommandResult, ConfigValueSource, CredentialRequest, CredentialSource, NeverCancelled,
    ProcessFuture, ProcessRunner, ResolvedCredential, SecretString, TokioProcessRunner,
    resolve_config_value, resolve_credential,
};
pub use error::{DiagnosticLevel, StoreDiagnostic, StoreError, StoreErrorCategory};
pub use models::{ModelSourceSnapshot, load_model_sources, strip_json_comments};
pub use paths::{StorePaths, canonicalize_for_match, expand_tilde, normalize_path};
pub use session::{
    SessionContext, SessionFile, SessionFileSnapshot, SessionIdentitySource, SessionModel,
    SessionRecordFactory, SessionStore, SessionWriter, SessionWriterState, StoreFuture,
};
pub use settings::{SettingsSnapshot, load_settings};
pub use trust::{
    ProtectedResource, ResourceAccess, TrustDecision, TrustDecisionSource, TrustDocument,
    TrustRequest, resolve_trust,
};
