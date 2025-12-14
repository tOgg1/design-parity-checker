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
            DpcError::Config(msg) => {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("playwright npm package is missing") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Install Playwright (e.g., `npm install playwright` and `npx playwright install chromium`).",
                    )
                } else if lower.contains("chromium executable") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Run `npx playwright install chromium` (or `playwright install chromium`) to download the browser.",
                    )
                } else if lower.contains("figma_token") || lower.contains("figma token") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Set FIGMA_TOKEN (or FIGMA_OAUTH_TOKEN) before running Figma inputs.",
                    )
                } else if lower.contains("node-id") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Include a Figma node-id in the URL (e.g., ?node-id=1-2) or pass --ref-type/--impl-type explicitly.",
                    )
                } else if lower.contains("file key") && lower.contains("figma") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Use a Figma URL with a file key: https://www.figma.com/file/<FILE_KEY>/... with node-id if needed.",
                    )
                } else if lower.contains("spawn playwright helper")
                    || lower.contains("node command")
                    || lower.contains("not found on path")
                {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Install Node.js and ensure the node binary is on PATH; rerun after installing Playwright if needed.",
                    )
                } else if lower.contains("timeout while waiting for navigation")
                    || lower.contains("network idle timeout")
                    || lower.contains("timeout")
                {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Try increasing --nav-timeout/--network-idle-timeout or ensure the page loads without blocking.",
                    )
                } else if lower.contains("unsupported file extension") {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Use a supported image type (png, jpg, jpeg, webp, gif) or override type with --ref-type/--impl-type.",
                    )
                } else if lower.contains("local file not found") || lower.contains("file not found")
                {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Verify the file exists; use an absolute path or run from the working directory, and ensure the extension is supported (png, jpg, jpeg, webp, gif).",
                    )
                } else {
                    ErrorPayload::new(
                        ErrorCategory::Config,
                        msg.to_string(),
                        "Check flags/paths (e.g., --viewport WIDTHxHEIGHT) and required tokens.",
                    )
                }
            }
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
            ImageLoadError::Ocr(msg) => DpcError::Metric(format!("OCR extraction failed: {}", msg)),
        }
    }
}

pub type Result<T> = std::result::Result<T, DpcError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_payload_includes_playwright_remediation() {
        let err = DpcError::Config(
            "Playwright npm package is missing; install with `npm install playwright`.".to_string(),
        );
        let payload = err.to_payload();
        assert_eq!(payload.category, ErrorCategory::Config);
        let remediation = payload.remediation.unwrap_or_default();
        assert!(
            remediation.contains("npm install playwright"),
            "expected remediation to mention npm install playwright, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_uses_default_remediation_for_other_messages() {
        let err = DpcError::Config("Some other config issue".to_string());
        let payload = err.to_payload();
        let remediation = payload.remediation.unwrap_or_default();
        assert!(
            remediation.contains("Check flags/paths"),
            "expected default remediation for generic config errors"
        );
    }

    #[test]
    fn config_payload_includes_figma_token_remediation() {
        let err = DpcError::Config("FIGMA_TOKEN environment variable is required".to_string());
        let payload = err.to_payload();
        let remediation = payload.remediation.unwrap_or_default();
        assert!(
            remediation.contains("FIGMA_TOKEN"),
            "expected FIGMA token remediation, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_node_install_hint() {
        let err = DpcError::Config(
            "Unable to spawn Playwright helper; 'node' was not found on PATH".to_string(),
        );
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation.to_ascii_lowercase().contains("node"),
            "expected node install/path remediation, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_timeout_hint() {
        let err = DpcError::Config(
            "Playwright error (status error): Timeout navigating to https://example.com"
                .to_string(),
        );
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation.contains("--nav-timeout")
                || remediation.to_ascii_lowercase().contains("timeout"),
            "expected timeout remediation, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_node_id_hint() {
        let err = DpcError::Config("Figma URL missing node-id in query".to_string());
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation.to_ascii_lowercase().contains("node-id"),
            "expected node-id remediation, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_file_key_hint() {
        let err = DpcError::Config("Figma URL missing file key".to_string());
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation.to_ascii_lowercase().contains("file key"),
            "expected file key remediation, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_file_not_found_hint() {
        let err = DpcError::Config(
            "Local file not found: missing.png. Hint: check the path relative to the current working directory or use an absolute path."
                .to_string(),
        );
        let remediation = err.to_payload().remediation.unwrap_or_default();
        let lower = remediation.to_ascii_lowercase();
        assert!(
            lower.contains("absolute path") || lower.contains("working directory"),
            "expected file path remediation, got: {remediation}"
        );
        assert!(
            remediation.contains("png") && remediation.contains("gif"),
            "expected supported extensions hint, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_lists_supported_extensions_for_unsupported_extension() {
        let err = DpcError::Config(
            "Unsupported file extension 'bmp'. Supported image extensions: png, jpg, jpeg, webp, gif.".to_string(),
        );
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation.contains("png") && remediation.contains("gif"),
            "expected remediation to list supported extensions, got: {remediation}"
        );
    }

    #[test]
    fn config_payload_includes_chromium_install_hint() {
        let err =
            DpcError::Config("chromium executable is missing; reinstall Playwright".to_string());
        let remediation = err.to_payload().remediation.unwrap_or_default();
        assert!(
            remediation
                .to_ascii_lowercase()
                .contains("playwright install chromium"),
            "expected remediation to mention playwright install chromium, got: {remediation}"
        );
    }
}
