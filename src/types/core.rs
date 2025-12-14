//! Core types used throughout the DPC library.
//!
//! This module contains the fundamental data structures:
//! - [`ResourceKind`] - Input type classification
//! - [`NormalizedView`] - Unified view representation
//! - [`BoundingBox`] - Element positioning
//! - [`TypographyStyle`] - Font properties
//! - [`OcrBlock`] - OCR-extracted text blocks

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use crate::viewport::Viewport;

use super::dom::DomSnapshot;
use super::figma::FigmaSnapshot;

/// Classification of input resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    /// Web URL to be rendered via Playwright
    Url,
    /// Local image file (PNG, JPG, WebP, etc.)
    Image,
    /// Figma design reference
    Figma,
}

/// A normalized representation of a design view.
///
/// This is the unified internal format that all input types (URL, image, Figma)
/// are converted to before metrics comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedView {
    /// The kind of resource this view was created from
    pub kind: ResourceKind,
    /// Path to the screenshot image
    pub screenshot_path: PathBuf,
    /// Width of the captured view in pixels
    pub width: u32,
    /// Height of the captured view in pixels
    pub height: u32,
    /// DOM snapshot (for URL inputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom: Option<DomSnapshot>,
    /// Figma node tree (for Figma inputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub figma_tree: Option<FigmaSnapshot>,
    /// OCR-extracted text blocks (for image inputs without DOM/Figma)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_blocks: Option<Vec<OcrBlock>>,
}

/// Rectangle bounds for an element.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Typography style properties.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TypographyStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<String>,
    pub line_height: Option<f32>,
}

/// A text block extracted via OCR.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrBlock {
    pub text: String,
    pub bounding_box: BoundingBox,
    pub confidence: Option<f32>,
}
