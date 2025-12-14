use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub use crate::viewport::Viewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Url,
    Image,
    Figma,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedView {
    pub kind: ResourceKind,
    pub screenshot_path: PathBuf,
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom: Option<DomSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub figma_tree: Option<FigmaSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_blocks: Option<Vec<OcrBlock>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomSnapshot {
    pub url: Option<String>,
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<DomNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomNode {
    pub id: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, String>,
    pub text: Option<String>,
    pub bounding_box: BoundingBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computed_style: Option<ComputedStyle>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TypographyStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<String>,
    pub line_height: Option<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSnapshot {
    pub file_key: String,
    pub node_id: String,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<FigmaNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaNode {
    pub id: String,
    pub name: Option<String>,
    pub node_type: String,
    pub bounding_box: BoundingBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<FigmaPaint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaPaint {
    pub kind: FigmaPaintKind,
    pub color: Option<String>,
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigmaPaintKind {
    Solid,
    Gradient,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrBlock {
    pub text: String,
    pub bounding_box: BoundingBox,
    pub confidence: Option<f32>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelMetric {
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff_regions: Vec<PixelDiffRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelDiffRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub severity: DiffSeverity,
    pub reason: PixelDiffReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSeverity {
    Minor,
    Moderate,
    Major,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelDiffReason {
    PixelChange,
    AntiAliasing,
    RenderingNoise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetric {
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff_regions: Vec<LayoutDiffRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDiffRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: LayoutDiffKind,
    pub element_type: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDiffKind {
    MissingElement,
    ExtraElement,
    PositionShift,
    SizeChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyMetric {
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<TypographyDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyDiff {
    pub element_id_ref: Option<String>,
    pub element_id_impl: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<TypographyIssue>,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypographyIssue {
    FontFamilyMismatch,
    FontSizeDiff,
    FontWeightDiff,
    LineHeightDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorMetric {
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<ColorDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorDiff {
    pub kind: ColorDiffKind,
    pub ref_color: String,
    pub impl_color: String,
    pub delta_e: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorDiffKind {
    PrimaryColorShift,
    AccentColorShift,
    BackgroundColorShift,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMetric {
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_text: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_text: Vec<String>,
}
