//! DOM snapshot types for browser-captured pages.
//!
//! These types represent the DOM structure extracted from web pages
//! via Playwright for structural comparison.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::core::BoundingBox;

/// A snapshot of a web page's DOM structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomSnapshot {
    /// The URL of the captured page
    pub url: Option<String>,
    /// The page title
    pub title: Option<String>,
    /// Flattened list of DOM nodes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<DomNode>,
}

/// A single DOM element with its properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomNode {
    /// Unique identifier for this node
    pub id: String,
    /// HTML tag name (e.g., "div", "span", "button")
    pub tag: String,
    /// IDs of child nodes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    /// ID of parent node
    pub parent: Option<String>,
    /// HTML attributes (id, class, data-*, etc.)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, String>,
    /// Text content (for text nodes)
    pub text: Option<String>,
    /// Position and size on screen
    pub bounding_box: BoundingBox,
    /// CSS computed styles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computed_style: Option<ComputedStyle>,
}

/// Computed CSS styles for a DOM element.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComputedStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<String>,
    pub line_height: Option<f32>,
    pub color: Option<String>,
    pub background_color: Option<String>,
    pub display: Option<String>,
    pub visibility: Option<String>,
    pub opacity: Option<f32>,
}
