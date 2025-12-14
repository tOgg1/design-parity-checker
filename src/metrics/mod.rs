//! Metrics module for comparing reference and implementation views.
//!
//! This module provides a unified interface for computing various design parity metrics:
//! - Pixel/perceptual similarity (SSIM-based)
//! - Layout/structure similarity (element matching)
//! - Typography similarity (font properties)
//! - Color palette similarity (k-means clustering)
//! - Content similarity (text matching)

// Submodules
mod color;
mod content;
mod issues;
mod layout;
mod pixel;
mod runner;
mod scoring;
mod typography;

#[cfg(test)]
mod tests;

// Re-exports
pub use color::ColorPaletteMetric;
pub use content::ContentSimilarity;
pub use issues::generate_top_issues;
pub use layout::LayoutSimilarity;
pub use pixel::{cluster_diff_regions, PixelDiffThresholds, PixelSimilarity};
pub use runner::{default_metrics, run_metrics, Metric, MetricKind, MetricResult};
pub use scoring::{calculate_combined_score, ScoreWeights};
pub use typography::TypographySimilarity;
