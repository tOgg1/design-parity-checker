use crate::error::ErrorPayload;
use crate::types::{MetricScores, ResourceKind, Viewport};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Schema version for output payloads.
pub const DPC_OUTPUT_VERSION: &str = "0.2.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum DpcOutput {
    Compare(CompareOutput),
    GenerateCode(GenerateCodeOutput),
    Quality(QualityOutput),
    Error(ErrorOutput),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    Compare,
    GenerateCode,
    Quality,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDescriptor {
    pub kind: ResourceKind,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareOutput {
    pub version: String,
    #[serde(rename = "ref")]
    pub ref_resource: ResourceDescriptor,
    #[serde(rename = "impl")]
    pub impl_resource: ResourceDescriptor,
    pub viewport: Viewport,
    pub similarity: f32,
    pub threshold: f32,
    pub passed: bool,
    pub metrics: MetricScores,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<CompareArtifacts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCodeOutput {
    pub version: String,
    pub input: ResourceDescriptor,
    pub viewport: Option<Viewport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOutput {
    pub version: String,
    pub input: ResourceDescriptor,
    pub viewport: Viewport,
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<QualityFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityFinding {
    pub severity: FindingSeverity,
    #[serde(rename = "type")]
    pub finding_type: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareArtifacts {
    pub directory: PathBuf,
    #[serde(default)]
    pub kept: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_screenshot: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impl_screenshot: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_image: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_dom_snapshot: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impl_dom_snapshot: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_figma_snapshot: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impl_figma_snapshot: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOutput {
    pub version: String,
    /// Convenience top-level message for pipelines that expect `message` alongside `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub error: ErrorPayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_output_serializes() {
        let output = DpcOutput::Compare(CompareOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            ref_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "ref.png".to_string(),
            },
            impl_resource: ResourceDescriptor {
                kind: ResourceKind::Url,
                value: "https://example.com".to_string(),
            },
            viewport: Viewport {
                width: 1440,
                height: 900,
            },
            similarity: 0.93,
            threshold: 0.9,
            passed: true,
            metrics: MetricScores {
                pixel: None,
                layout: None,
                typography: None,
                color: None,
                content: None,
            },
            summary: Some(Summary {
                top_issues: vec!["Minor color shift".into()],
            }),
            artifacts: None,
        });

        let json = serde_json::to_string(&output).expect("serialize compare output");
        assert!(json.contains("\"mode\":\"compare\""));
        assert!(json.contains("\"similarity\":0.93"));
    }

    #[test]
    fn compare_output_with_artifacts_serializes() {
        let artifacts = CompareArtifacts {
            directory: PathBuf::from("/tmp/dpc-123"),
            kept: true,
            ref_screenshot: Some(PathBuf::from("/tmp/dpc-123/ref.png")),
            impl_screenshot: Some(PathBuf::from("/tmp/dpc-123/impl.png")),
            diff_image: Some(PathBuf::from("/tmp/dpc-123/diff.png")),
            ref_dom_snapshot: None,
            impl_dom_snapshot: Some(PathBuf::from("/tmp/dpc-123/impl_dom.json")),
            ref_figma_snapshot: None,
            impl_figma_snapshot: None,
        };

        let output = DpcOutput::Compare(CompareOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            ref_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "ref.png".to_string(),
            },
            impl_resource: ResourceDescriptor {
                kind: ResourceKind::Url,
                value: "https://example.com".to_string(),
            },
            viewport: Viewport {
                width: 1440,
                height: 900,
            },
            similarity: 0.93,
            threshold: 0.9,
            passed: true,
            metrics: MetricScores {
                pixel: None,
                layout: None,
                typography: None,
                color: None,
                content: None,
            },
            summary: None,
            artifacts: Some(artifacts),
        });

        let json = serde_json::to_string(&output).expect("serialize compare output");
        assert!(json.contains("\"artifacts\""));
        assert!(json.contains("/tmp/dpc-123/ref.png"));
    }

    #[test]
    fn generate_output_serializes() {
        let output = DpcOutput::GenerateCode(GenerateCodeOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            input: ResourceDescriptor {
                kind: ResourceKind::Figma,
                value: "figma-file".to_string(),
            },
            viewport: Some(Viewport {
                width: 1280,
                height: 720,
            }),
            stack: Some("html+tailwind".to_string()),
            output_path: Some(PathBuf::from("output.html")),
            code: Some("<div>hi</div>".to_string()),
            summary: None,
        });

        let json = serde_json::to_string(&output).expect("serialize generate output");
        assert!(json.contains("\"mode\":\"generate-code\""));
        assert!(json.contains("\"stack\":\"html+tailwind\""));
    }

    #[test]
    fn quality_output_serializes() {
        let output = DpcOutput::Quality(QualityOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            input: ResourceDescriptor {
                kind: ResourceKind::Url,
                value: "https://example.com".to_string(),
            },
            viewport: Viewport {
                width: 1024,
                height: 768,
            },
            score: 0.82,
            findings: vec![QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: "typography".to_string(),
                message: "Font weight mismatch".to_string(),
            }],
        });

        let json = serde_json::to_string(&output).expect("serialize quality output");
        assert!(json.contains("\"mode\":\"quality\""));
        assert!(json.contains("\"score\":0.82"));
        assert!(json.contains("\"severity\":\"warning\""));
    }

    #[test]
    fn error_output_serializes() {
        let output = DpcOutput::Error(ErrorOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            message: Some("missing ref".to_string()),
            error: crate::error::ErrorPayload::new(
                crate::error::ErrorCategory::Config,
                "missing ref".to_string(),
                "provide --ref",
            ),
        });

        let json = serde_json::to_string(&output).expect("serialize error output");
        assert!(json.contains("\"mode\":\"error\""));
        assert!(json.contains("\"missing ref\""));
        assert!(json.contains("\"message\""));
    }
}
