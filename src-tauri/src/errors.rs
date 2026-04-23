use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    Unauthorized,
    Network,
    Timeout,
    InvalidResponse,
    MissingHeaders,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            ProviderErrorKind::Unauthorized => "unauthorized",
            ProviderErrorKind::Network => "network",
            ProviderErrorKind::Timeout => "timeout",
            ProviderErrorKind::InvalidResponse => "invalid_response",
            ProviderErrorKind::MissingHeaders => "missing_headers",
            ProviderErrorKind::Unknown => "unknown",
        };
        write!(f, "[{kind}] {}", self.message)
    }
}

impl std::error::Error for ProviderError {}
