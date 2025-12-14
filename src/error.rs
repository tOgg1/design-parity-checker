use crate::image_loader::ImageLoadError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::ParseError;

#[derive(Debug, Error)]
pub enum DpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] ParseError),

    #[error("Figma API error (status: {status:?}): {message}")]
    FigmaApi {
        status: Option<StatusCode>,
        message: String,
    },

    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Metric computation error: {0}")]
    Metric(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Unexpected error: {0}")]
    Unknown(String),
}

impl DpcError {
    pub fn figma_api(status: Option<StatusCode>, message: impl Into<String>) -> Self {
        DpcError::FigmaApi {
            status,
            message: message.into(),
        }
    }

    pub fn metric(message: impl Into<String>) -> Self {
        DpcError::Metric(message.into())
    }

    pub fn to_payload(&self) -> ErrorPayload {
        match self {
            DpcError::Io(e) => ErrorPayload::new(
                ErrorCategory::Config,
                e.to_string(),
                "Check file paths/permissions.",
            ),
            DpcError::Network(e) => ErrorPayload::new(
                ErrorCategory::Network,
                e.to_string(),
                "Check connectivity/proxy/VPN and retry.",
            ),
            DpcError::InvalidUrl(e) => ErrorPayload::new(
                ErrorCategory::Config,
                e.to_string(),
                "Verify URL/format (e.g., https://example.com).",
            ),
            DpcError::FigmaApi { status, message } => ErrorPayload::new(
                ErrorCategory::Figma,
                format!("Figma API error (status {:?}): {}", status, message),
                "Check FIGMA_TOKEN/URL and rate limits; retry after waiting.",
            ),
            DpcError::Image(e) => ErrorPayload::new(
                ErrorCategory::Image,
                e.to_string(),
                "Verify image path/format and readability.",
            ),
            DpcError::Serialization(e) => ErrorPayload::new(
                ErrorCategory::Config,
                e.to_string(),
                "Check JSON/serialization inputs; run with --verbose for details.",
            ),
            DpcError::Metric(msg) => ErrorPayload::new(
                ErrorCategory::Metric,
                msg.to_string(),
                "Inspect metric inputs; try rerunning with --verbose.",
            ),
            DpcError::Config(msg) => ErrorPayload::new(
                ErrorCategory::Config,
                msg.to_string(),
                "Check flags/paths (e.g., --viewport WIDTHxHEIGHT) and required tokens.",
            ),
            DpcError::Unknown(msg) => ErrorPayload::new(
                ErrorCategory::Unknown,
                msg.to_string(),
                "Re-run with --verbose; file an issue if persistent.",
            ),
        }
    }
}

impl From<ImageLoadError> for DpcError {
    fn from(err: ImageLoadError) -> Self {
        match err {
            ImageLoadError::Load(e) => DpcError::Image(e),
            ImageLoadError::NotFound(path) => DpcError::Config(format!("File not found: {}", path)),
            ImageLoadError::Save(msg) => DpcError::Io(std::io::Error::other(format!(
                "Failed to save image: {}",
                msg
            ))),
        }
    }
}

pub type Result<T> = std::result::Result<T, DpcError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorCategory {
    Config,
    Network,
    Figma,
    Image,
    Metric,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub category: ErrorCategory,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl ErrorPayload {
    pub fn new(category: ErrorCategory, message: String, remediation: impl Into<String>) -> Self {
        Self {
            category,
            message,
            remediation: Some(remediation.into()),
        }
    }
}
