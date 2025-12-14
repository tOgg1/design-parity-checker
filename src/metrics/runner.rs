use crate::error::DpcError;
use crate::types::{
    ColorMetric, ContentMetric, LayoutMetric, MetricScores, NormalizedView, PixelMetric,
    TypographyMetric,
};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::{
    ColorPaletteMetric, ContentSimilarity, LayoutSimilarity, PixelSimilarity, TypographySimilarity,
};

/// The kind of metric being computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Pixel,
    Layout,
    Typography,
    Color,
    Content,
}

impl MetricKind {
    pub const fn all() -> [MetricKind; 5] {
        [
            MetricKind::Pixel,
            MetricKind::Layout,
            MetricKind::Typography,
            MetricKind::Color,
            MetricKind::Content,
        ]
    }
}

impl fmt::Display for MetricKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MetricKind::Pixel => "pixel",
                MetricKind::Layout => "layout",
                MetricKind::Typography => "typography",
                MetricKind::Color => "color",
                MetricKind::Content => "content",
            }
        )
    }
}

impl FromStr for MetricKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pixel" => Ok(MetricKind::Pixel),
            "layout" => Ok(MetricKind::Layout),
            "typography" => Ok(MetricKind::Typography),
            "color" => Ok(MetricKind::Color),
            "content" => Ok(MetricKind::Content),
            other => Err(format!("unknown metric kind: {}", other)),
        }
    }
}

/// Trait for implementing a design parity metric.
pub trait Metric {
    fn kind(&self) -> MetricKind;
    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult>;
}

/// Result of a metric computation, containing the specific metric data.
#[derive(Debug, Clone)]
pub enum MetricResult {
    Pixel(PixelMetric),
    Layout(LayoutMetric),
    Typography(TypographyMetric),
    Color(ColorMetric),
    Content(ContentMetric),
}

impl MetricResult {
    pub fn kind(&self) -> MetricKind {
        match self {
            MetricResult::Pixel(_) => MetricKind::Pixel,
            MetricResult::Layout(_) => MetricKind::Layout,
            MetricResult::Typography(_) => MetricKind::Typography,
            MetricResult::Color(_) => MetricKind::Color,
            MetricResult::Content(_) => MetricKind::Content,
        }
    }

    pub fn score(&self) -> f32 {
        match self {
            MetricResult::Pixel(m) => m.score,
            MetricResult::Layout(m) => m.score,
            MetricResult::Typography(m) => m.score,
            MetricResult::Color(m) => m.score,
            MetricResult::Content(m) => m.score,
        }
    }
}

/// Returns the default set of all metrics.
pub fn default_metrics() -> Vec<Box<dyn Metric>> {
    vec![
        Box::new(PixelSimilarity::default()),
        Box::new(LayoutSimilarity::default()),
        Box::new(TypographySimilarity::default()),
        Box::new(ColorPaletteMetric::default()),
        Box::new(ContentSimilarity::default()),
    ]
}

fn has_layout_data(view: &NormalizedView) -> bool {
    view.dom
        .as_ref()
        .map(|d| !d.nodes.is_empty())
        .unwrap_or(false)
        || view
            .figma_tree
            .as_ref()
            .map(|f| !f.nodes.is_empty())
            .unwrap_or(false)
}

fn has_typography_data(view: &NormalizedView) -> bool {
    if let Some(dom) = &view.dom {
        if dom
            .nodes
            .iter()
            .any(|n| n.text.is_some() && n.computed_style.is_some())
        {
            return true;
        }
    }
    if let Some(figma) = &view.figma_tree {
        if figma
            .nodes
            .iter()
            .any(|n| n.text.is_some() && n.typography.is_some())
        {
            return true;
        }
    }
    false
}

fn has_content_data(view: &NormalizedView) -> bool {
    if let Some(dom) = &view.dom {
        if dom.nodes.iter().any(|n| {
            n.text
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
        }) {
            return true;
        }
    }
    if let Some(figma) = &view.figma_tree {
        if figma.nodes.iter().any(|n| {
            n.text
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
        }) {
            return true;
        }
    }
    if let Some(blocks) = &view.ocr_blocks {
        if !blocks.is_empty() {
            return true;
        }
    }
    false
}

/// Run the specified metrics on the reference and implementation views.
pub fn run_metrics(
    metrics: &[Box<dyn Metric>],
    selected: &[MetricKind],
    reference: &NormalizedView,
    implementation: &NormalizedView,
) -> Result<MetricScores> {
    let desired: Vec<MetricKind> = if selected.is_empty() {
        MetricKind::all().to_vec()
    } else {
        selected.to_vec()
    };

    let layout_available = has_layout_data(reference);
    let typography_available =
        has_typography_data(reference) && has_typography_data(implementation);
    let content_available = has_content_data(reference) && has_content_data(implementation);

    let missing: Vec<MetricKind> = desired
        .iter()
        .copied()
        .filter(|kind| !metrics.iter().any(|m| m.kind() == *kind))
        .collect();

    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(MetricKind::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(DpcError::Config(format!(
            "Requested metrics not available: {}",
            names
        )));
    }

    let mut scores = MetricScores {
        pixel: None,
        layout: None,
        typography: None,
        color: None,
        content: None,
    };

    for metric in metrics {
        let kind = metric.kind();
        if !desired.contains(&kind) {
            continue;
        }

        if matches!(kind, MetricKind::Layout) && !layout_available {
            continue;
        }
        if matches!(kind, MetricKind::Typography) && !typography_available {
            continue;
        }
        if matches!(kind, MetricKind::Content) && !content_available {
            continue;
        }

        let result = metric.compute(reference, implementation)?;
        match result {
            MetricResult::Pixel(m) => scores.pixel = Some(m),
            MetricResult::Layout(m) => scores.layout = Some(m),
            MetricResult::Typography(m) => scores.typography = Some(m),
            MetricResult::Color(m) => scores.color = Some(m),
            MetricResult::Content(m) => scores.content = Some(m),
        }
    }

    Ok(scores)
}
