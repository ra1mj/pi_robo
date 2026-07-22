//! Provider adapter boundary and shared transport-facing contracts.

mod anthropic;
mod clock;
mod common;
mod config;
mod error;
mod google;
mod openai_chat;
mod openai_responses;
mod sse;
mod transport;

pub use anthropic::AnthropicMessagesAdapter;
pub use clock::{ProviderClock, SystemProviderClock};
pub use config::{ProviderAdapterConfig, ProviderTimeouts, SecretString};
pub use error::{ProviderErrorContext, ProviderFailureKind, normalize_provider_error};
pub use google::GoogleGenerativeLanguageAdapter;
pub use openai_chat::OpenAiChatAdapter;
pub use openai_responses::OpenAiResponsesAdapter;
pub use pi_model::{
    CacheRetention, Cancellation, ModelEventStream, ModelRequest, ModelRequestOptions,
    ModelService, ModelServiceError, ModelServiceErrorCategory, ThinkingBudgets, ThinkingLevel,
    ToolChoice,
};
pub use pi_protocol::ProviderId;
pub use sse::{SseDecoder, SseEvent};
pub use transport::{BoundedBody, ProviderHttpClient, ProviderHttpResponse};
