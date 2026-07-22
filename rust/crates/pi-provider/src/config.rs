use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::time::Duration;

/// Sensitive provider configuration value with redacted debug output.
#[derive(Clone, Eq, PartialEq)]
pub struct SecretString(String);

impl SecretString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl Debug for SecretString {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

/// Timeout boundaries applied independently to provider transport phases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderTimeouts {
    connect: Duration,
    response_header: Duration,
    body_idle: Duration,
}

impl ProviderTimeouts {
    #[must_use]
    pub const fn new(connect: Duration, response_header: Duration, body_idle: Duration) -> Self {
        Self {
            connect,
            response_header,
            body_idle,
        }
    }

    #[must_use]
    pub const fn connect(self) -> Duration {
        self.connect
    }

    #[must_use]
    pub const fn response_header(self) -> Duration {
        self.response_header
    }

    #[must_use]
    pub const fn body_idle(self) -> Duration {
        self.body_idle
    }
}

/// Fully resolved immutable configuration for one provider adapter.
#[derive(Clone)]
pub struct ProviderAdapterConfig {
    base_url: String,
    authorization: Option<SecretString>,
    headers: BTreeMap<String, String>,
    proxy: Option<SecretString>,
    timeouts: ProviderTimeouts,
}

impl ProviderAdapterConfig {
    #[must_use]
    pub fn new(base_url: impl Into<String>, timeouts: ProviderTimeouts) -> Self {
        Self {
            base_url: base_url.into(),
            authorization: None,
            headers: BTreeMap::new(),
            proxy: None,
            timeouts,
        }
    }

    #[must_use]
    pub fn with_authorization(mut self, authorization: SecretString) -> Self {
        self.authorization = Some(authorization);
        self
    }

    #[must_use]
    pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    #[must_use]
    pub fn with_default_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.headers
            .entry(name.into())
            .or_insert_with(|| value.into());
        self
    }

    #[must_use]
    pub fn with_proxy(mut self, proxy: SecretString) -> Self {
        self.proxy = Some(proxy);
        self
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn authorization(&self) -> Option<&SecretString> {
        self.authorization.as_ref()
    }

    #[must_use]
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    #[must_use]
    pub fn proxy(&self) -> Option<&SecretString> {
        self.proxy.as_ref()
    }

    #[must_use]
    pub const fn timeouts(&self) -> ProviderTimeouts {
        self.timeouts
    }
}
