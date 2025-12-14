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

use crate::types::core::BoundingBox;

/// Represents the results of the Hierarchy metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HierarchyMetric {
    pub score: f64,
    pub issues: Vec<HierarchyIssue>,
    pub distinct_tiers: Vec<f64>, // List of distinct font sizes found
    pub tier_count: usize,       // Number of distinct tiers
}

/// Represents an issue found by the Hierarchy metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HierarchyIssue {
    /// Too many distinct font size tiers detected, indicating lack of clear hierarchy.
    TooManyTiers(usize),
    /// Too few distinct font size tiers detected, indicating insufficient hierarchy.
    TooFewTiers(usize),
    /// A text element has an unusual font size that doesn't fit into established tiers.
    #[serde(skip_serializing_if = "Option::is_none")]
    UnusualFontSize {
        font_size: f64,
        element_text: Option<String>,
        bounding_box: BoundingBox,
    },
}

/// Container for all metric scores.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<HierarchyMetric>, // New field for HierarchyMetric
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
    pub score: f64,
    pub issues: Vec<LayoutIssue>,
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

/// Represents an issue found by the Layout metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutIssue {
    /// An element present in the reference is missing in the implementation.
    MissingElement {
        element_type: Option<String>,
        bounding_box: BoundingBox,
    },
    /// An element present in the implementation is not found in the reference.
    ExtraElement {
        element_type: Option<String>,
        bounding_box: BoundingBox,
    },
    /// An element has significantly shifted position.
    PositionShift {
        element_type: Option<String>,
        ref_box: BoundingBox,
        impl_box: BoundingBox,
    },
    /// An element has significantly changed size.
    SizeChange {
        element_type: Option<String>,
        ref_box: BoundingBox,
        impl_box: BoundingBox,
    },
}

// ============================================================================
// Typography Metric Types
// ============================================================================

/// Result of typography comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyMetric {
    pub score: f64,
    pub issues: Vec<TypographyIssue>,
}

/// Represents an issue found by the Typography metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypographyIssue {
    FontFamilyMismatch,
    FontSizeDiff,
    FontWeightDiff,
    LineHeightDiff,
    LetterSpacingDiff,
    TextAlignDiff,
}

// ============================================================================
// Color Metric Types
// ============================================================================

/// Result of color palette comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorMetric {
    pub score: f64,
    pub issues: Vec<ColorIssue>,
}

/// Represents an issue found by the Color metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorIssue {
    /// A primary color in the reference palette is missing or significantly shifted in the implementation.
    PrimaryColorShift {
        ref_color: String,
        impl_color: Option<String>,
        delta_e: Option<f32>,
    },
    /// An accent color in the reference palette is missing or significantly shifted in the implementation.
    AccentColorShift {
        ref_color: String,
        impl_color: Option<String>,
        delta_e: Option<f32>,
    },
    /// A background color in the reference palette is missing or significantly shifted in the implementation.
    BackgroundColorShift {
        ref_color: String,
        impl_color: Option<String>,
        delta_e: Option<f32>,
    },
    /// The overall number of colors in the implementation deviates significantly from the reference.
    PaletteCountMismatch {
        ref_count: usize,
        impl_count: usize,
    },
}

// ============================================================================
// Content Metric Types
// ============================================================================

/// Result of content/text comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMetric {
    pub score: f64,
    pub issues: Vec<ContentIssue>,
}

/// Represents an issue found by the Content metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentIssue {
    MissingText,
    ExtraText,
}

// ============================================================================
// MetricResult Enum
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)] // To allow for flexible deserialization
pub enum MetricResult {
    Pixel(PixelMetric),
    Layout(LayoutMetric),
    Typography(TypographyMetric),
    Color(ColorMetric),
    Content(ContentMetric),
    Hierarchy(HierarchyMetric),
}
