//! Data types used throughout the DPC library.
//!
//! This module is organized by domain:
//! - [`core`] - Core types (NormalizedView, ResourceKind, BoundingBox)
//! - [`dom`] - DOM snapshot types for browser captures
//! - [`figma`] - Figma design snapshot types
//! - [`metric_results`] - Metric result types for comparison output

pub mod core;
pub mod dom;
pub mod figma;
pub mod metric_results;

// Re-export core types at module level for convenience
pub use core::{BoundingBox, NormalizedView, OcrBlock, ResourceKind, TypographyStyle, Viewport};

// Re-export DOM types
pub use dom::{ComputedStyle, DomNode, DomSnapshot};

// Re-export Figma types
pub use figma::{FigmaNode, FigmaPaint, FigmaPaintKind, FigmaSnapshot};

// Re-export metric types
pub use metric_results::{
    ColorDiff, ColorDiffKind, ColorMetric, ContentMetric, DiffSeverity, LayoutDiffKind,
    LayoutDiffRegion, LayoutMetric, MetricScores, PixelDiffReason, PixelDiffRegion, PixelMetric,
    SemanticDiff, SemanticDiffType, TypographyDiff, TypographyIssue, TypographyMetric,
};
