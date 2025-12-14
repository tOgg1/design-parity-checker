//! Metric result types for comparison output.
//!
//! These types represent the results of various parity metrics:
//! - Pixel similarity (SSIM-based)
//! - Layout comparison (element matching)
//! - Typography comparison (font properties)
//! - Color palette comparison
//! - Content comparison (text matching)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Container for all metric scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricScores {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel: Option<PixelMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentMetric>,
}

// ============================================================================
// Pixel Metric Types
// ============================================================================

/// Result of pixel/perceptual similarity comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelMetric {
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Regions where differences were detected
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff_regions: Vec<PixelDiffRegion>,
}

/// A region of pixel differences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelDiffRegion {
    /// X position (normalized 0.0 - 1.0)
    pub x: f32,
    /// Y position (normalized 0.0 - 1.0)
    pub y: f32,
    /// Width (normalized 0.0 - 1.0)
    pub width: f32,
    /// Height (normalized 0.0 - 1.0)
    pub height: f32,
    /// How significant the difference is
    pub severity: DiffSeverity,
    /// Why this difference was flagged
    pub reason: PixelDiffReason,
}

/// Severity level of a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSeverity {
    Minor,
    Moderate,
    Major,
}

/// Reason for a pixel difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelDiffReason {
    PixelChange,
    AntiAliasing,
    RenderingNoise,
}

// ============================================================================
// Layout Metric Types
// ============================================================================

/// Result of layout/structure comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetric {
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Regions with layout differences
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff_regions: Vec<LayoutDiffRegion>,
}

/// A layout difference region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDiffRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Type of layout difference
    pub kind: LayoutDiffKind,
    /// Element type (e.g., "div", "TEXT")
    pub element_type: Option<String>,
    /// Human-readable label
    pub label: Option<String>,
}

/// Type of layout difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDiffKind {
    MissingElement,
    ExtraElement,
    PositionShift,
    SizeChange,
}

// ============================================================================
// Typography Metric Types
// ============================================================================

/// Result of typography comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyMetric {
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Typography differences found
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<TypographyDiff>,
}

/// A typography difference between elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyDiff {
    /// Element ID in reference
    pub element_id_ref: Option<String>,
    /// Element ID in implementation
    pub element_id_impl: Option<String>,
    /// List of typography issues
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<TypographyIssue>,
    /// Additional details (ref/impl values)
    pub details: Option<Value>,
}

/// Type of typography issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypographyIssue {
    FontFamilyMismatch,
    FontSizeDiff,
    FontWeightDiff,
    LineHeightDiff,
}

// ============================================================================
// Color Metric Types
// ============================================================================

/// Result of color palette comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorMetric {
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Color differences found
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<ColorDiff>,
}

/// A color difference between palettes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorDiff {
    /// Type of color shift
    pub kind: ColorDiffKind,
    /// Reference color (hex)
    pub ref_color: String,
    /// Implementation color (hex)
    pub impl_color: String,
    /// Delta E (perceptual difference)
    pub delta_e: Option<f32>,
}

/// Type of color difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorDiffKind {
    PrimaryColorShift,
    AccentColorShift,
    BackgroundColorShift,
}

// ============================================================================
// Content Metric Types
// ============================================================================

/// Result of content/text comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMetric {
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Text present in reference but missing in implementation
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_text: Vec<String>,
    /// Text present in implementation but not in reference
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_text: Vec<String>,
}
