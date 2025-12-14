//! Figma design snapshot types.
//!
//! These types represent the Figma node structure extracted from
//! Figma designs via the Figma API for structural comparison.

use serde::{Deserialize, Serialize};

use super::core::{BoundingBox, TypographyStyle};

/// A snapshot of a Figma design frame/component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSnapshot {
    /// The Figma file key
    pub file_key: String,
    /// The node ID within the file
    pub node_id: String,
    /// The node name
    pub name: Option<String>,
    /// Flattened list of Figma nodes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<FigmaNode>,
}

/// A single Figma design node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaNode {
    /// Unique node ID
    pub id: String,
    /// Node name in Figma
    pub name: Option<String>,
    /// Figma node type (FRAME, TEXT, RECTANGLE, etc.)
    pub node_type: String,
    /// Position and size
    pub bounding_box: BoundingBox,
    /// Text content (for TEXT nodes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Typography properties (for TEXT nodes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyStyle>,
    /// Fill paints applied to this node
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<FigmaPaint>,
    /// IDs of child nodes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
}

/// A Figma paint/fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaPaint {
    /// Type of paint
    pub kind: FigmaPaintKind,
    /// Color in hex format (for solid fills)
    pub color: Option<String>,
    /// Opacity (0.0 - 1.0)
    pub opacity: Option<f32>,
}

/// Types of Figma paint fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigmaPaintKind {
    Solid,
    Gradient,
    Image,
}
