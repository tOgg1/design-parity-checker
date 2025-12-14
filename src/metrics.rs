//! Metrics module for comparing reference and implementation views.
//!
//! This module provides a unified interface for computing various design parity metrics:
//! - Pixel/perceptual similarity (SSIM-based)
//! - Layout/structure similarity (element matching)
//! - Typography similarity (font properties)
//! - Color palette similarity (k-means clustering)
//! - Content similarity (text matching)

use crate::error::DpcError;
use crate::types::{
    ColorMetric, ContentMetric, LayoutMetric, MetricScores, NormalizedView, PixelMetric,
    TypographyMetric,
};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// Submodules
mod color;
mod content;
mod issues;
mod layout;
mod pixel;
mod scoring;
mod typography;

// Re-exports
pub use color::ColorPaletteMetric;
pub use content::ContentSimilarity;
pub use issues::generate_top_issues;
pub use layout::LayoutSimilarity;
pub use pixel::{cluster_diff_regions, PixelDiffThresholds, PixelSimilarity};
pub use scoring::{calculate_combined_score, ScoreWeights};
pub use typography::TypographySimilarity;

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

// Implement Metric trait for each metric type

impl Metric for PixelSimilarity {
    fn kind(&self) -> MetricKind {
        MetricKind::Pixel
    }

    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult> {
        let metric = PixelSimilarity::compute_metric(self, reference, implementation)?;
        Ok(MetricResult::Pixel(metric))
    }
}

impl Metric for LayoutSimilarity {
    fn kind(&self) -> MetricKind {
        MetricKind::Layout
    }

    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult> {
        let metric = LayoutSimilarity::compute_metric(self, reference, implementation)?;
        Ok(MetricResult::Layout(metric))
    }
}

impl Metric for TypographySimilarity {
    fn kind(&self) -> MetricKind {
        MetricKind::Typography
    }

    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult> {
        let metric = TypographySimilarity::compute_metric(self, reference, implementation)?;
        Ok(MetricResult::Typography(metric))
    }
}

impl Metric for ColorPaletteMetric {
    fn kind(&self) -> MetricKind {
        MetricKind::Color
    }

    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult> {
        let metric = ColorPaletteMetric::compute_metric(self, reference, implementation)?;
        Ok(MetricResult::Color(metric))
    }
}

impl Metric for ContentSimilarity {
    fn kind(&self) -> MetricKind {
        MetricKind::Content
    }

    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult> {
        let metric = ContentSimilarity::compute_metric(self, reference, implementation)?;
        Ok(MetricResult::Content(metric))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ColorMetric, ComputedStyle, ContentMetric, LayoutMetric, PixelDiffReason, PixelDiffRegion,
        PixelMetric, ResourceKind, TypographyMetric, TypographyStyle, DiffSeverity,
        LayoutDiffRegion, LayoutDiffKind, ColorDiff, ColorDiffKind, TypographyDiff, TypographyIssue,
    };
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::cell::RefCell;
    use std::rc::Rc;
    use tempfile::NamedTempFile;

    #[test]
    fn metric_kind_display_and_parse_round_trip() {
        for kind in MetricKind::all() {
            let rendered = kind.to_string();
            let parsed = MetricKind::from_str(&rendered).expect("parse should succeed");
            assert_eq!(parsed, kind);
        }

        let parsed = MetricKind::from_str("LAYOUT").expect("case insensitive parse");
        assert_eq!(parsed, MetricKind::Layout);

        assert!(MetricKind::from_str("unknown").is_err());
    }

    #[test]
    fn run_metrics_errors_when_defaults_missing() {
        let ref_view = dummy_view();
        let impl_view = dummy_view();
        let metrics: Vec<Box<dyn Metric>> =
            vec![Box::new(StubMetric::pixel(0.9, Rc::new(RefCell::new(0))))];

        let err = run_metrics(&metrics, &[], &ref_view, &impl_view).unwrap_err();
        let msg = format!("{}", err);

        assert!(
            msg.contains("Requested metrics not available"),
            "expected missing metrics message, got: {}",
            msg
        );
        assert!(msg.contains("layout"));
        assert!(msg.contains("typography"));
        assert!(msg.contains("color"));
        assert!(msg.contains("content"));
    }

    #[test]
    fn run_metrics_errors_when_selected_missing() {
        let ref_view = dummy_view();
        let impl_view = dummy_view();
        let metrics: Vec<Box<dyn Metric>> = vec![];

        let err = run_metrics(&metrics, &[MetricKind::Pixel], &ref_view, &impl_view).unwrap_err();
        let msg = format!("{}", err);

        assert!(msg.contains("Requested metrics not available: pixel"));
    }

    #[test]
    fn run_metrics_executes_only_selected_metrics() {
        let ref_view = dummy_view();
        let impl_view = dummy_view();

        let pixel_calls = Rc::new(RefCell::new(0));
        let layout_calls = Rc::new(RefCell::new(0));

        let metrics: Vec<Box<dyn Metric>> = vec![
            Box::new(StubMetric::pixel(0.8, pixel_calls.clone())),
            Box::new(StubMetric::layout(0.7, layout_calls.clone())),
        ];

        let scores = run_metrics(&metrics, &[MetricKind::Pixel], &ref_view, &impl_view)
            .expect("should succeed");

        assert_eq!(*pixel_calls.borrow(), 1);
        assert_eq!(*layout_calls.borrow(), 0, "layout metric should be skipped");
        assert!(scores.pixel.is_some());
        assert!(scores.layout.is_none());
    }

    #[test]
    fn run_metrics_returns_scores_for_selected_metric() {
        let ref_view = dummy_view();
        let impl_view = dummy_view();
        let metrics: Vec<Box<dyn Metric>> =
            vec![Box::new(StubMetric::pixel(0.92, Rc::new(RefCell::new(0))))];

        let scores = run_metrics(&metrics, &[MetricKind::Pixel], &ref_view, &impl_view)
            .expect("should succeed");

        let pixel = scores.pixel.expect("pixel metric should be present");
        assert_eq!(pixel.score, 0.92);
        assert!(scores.layout.is_none());
        assert!(scores.typography.is_none());
        assert!(scores.color.is_none());
        assert!(scores.content.is_none());
    }

    #[test]
    fn run_metrics_skips_structural_when_no_structure() {
        let ref_img = solid_image([10, 20, 30, 255]);
        let impl_img = solid_image([10, 20, 30, 255]);
        let ref_view = view_from_file(ref_img.path(), 4, 4);
        let impl_view = view_from_file(impl_img.path(), 4, 4);
        let metrics = default_metrics();

        let scores = run_metrics(&metrics, &[], &ref_view, &impl_view)
            .expect("should skip structural metrics");

        assert!(scores.pixel.is_some());
        assert!(scores.color.is_some());
        assert!(scores.layout.is_none());
        assert!(scores.typography.is_none());
        assert!(scores.content.is_none());
    }

    #[test]
    fn run_metrics_scores_layout_even_when_impl_is_empty() {
        let ref_view = view_with_dom(vec![("button", bbox(0.0, 0.0, 0.5, 0.5))]);
        let impl_view = view_with_dom(vec![]);
        let scores = run_metrics(
            &default_metrics(),
            &[MetricKind::Layout],
            &ref_view,
            &impl_view,
        )
        .expect("layout metric should return a low score instead of erroring");

        let layout = scores
            .layout
            .expect("layout metric should be present even when implementation has no elements");
        assert!(
            layout.score <= 0.05,
            "layout score should be near zero when implementation is empty: {}",
            layout.score
        );
        assert!(
            !layout.diff_regions.is_empty(),
            "missing elements should be reported as diff regions"
        );
    }

    #[test]
    fn combined_score_rescales_to_present_metrics() {
        let weights = ScoreWeights {
            pixel: 0.7,
            layout: 0.3,
            typography: 0.2,
            color: 0.1,
            content: 0.1,
        };

        let scores_pixel_only = MetricScores {
            pixel: Some(PixelMetric {
                score: 0.4,
                diff_regions: vec![],
            }),
            layout: None,
            typography: None,
            color: None,
            content: None,
        };

        let combined_pixel = calculate_combined_score(&scores_pixel_only, &weights);
        assert!((combined_pixel - 0.4).abs() < f32::EPSILON);

        let mut scores_with_layout = scores_pixel_only;
        scores_with_layout.layout = Some(LayoutMetric {
            score: 0.8,
            diff_regions: vec![],
        });
        let combined_with_layout = calculate_combined_score(&scores_with_layout, &weights);
        assert!((combined_with_layout - 0.52).abs() < 1e-6);
    }

    #[test]
    fn combined_score_handles_zero_weights_and_missing_metrics() {
        let empty_scores = MetricScores {
            pixel: None,
            layout: None,
            typography: None,
            color: None,
            content: None,
        };
        let zero_result = calculate_combined_score(&empty_scores, &ScoreWeights::default());
        assert_eq!(zero_result, 0.0);

        let scores = MetricScores {
            pixel: Some(PixelMetric {
                score: 1.0,
                diff_regions: vec![],
            }),
            layout: Some(LayoutMetric {
                score: 0.25,
                diff_regions: vec![],
            }),
            typography: None,
            color: None,
            content: None,
        };
        let weights = ScoreWeights {
            pixel: 0.0,
            layout: 1.0,
            typography: 0.0,
            color: 0.0,
            content: 0.0,
        };
        let combined = calculate_combined_score(&scores, &weights);
        assert!((combined - 0.25).abs() < 1e-6);
    }

    #[test]
    fn generate_top_issues_orders_by_severity_and_limits_count() {
        let scores = MetricScores {
            pixel: Some(PixelMetric {
                score: 0.4,
                diff_regions: vec![PixelDiffRegion {
                    x: 0.0,
                    y: 0.0,
                    width: 0.2,
                    height: 0.2,
                    severity: DiffSeverity::Minor,
                    reason: PixelDiffReason::PixelChange,
                }],
            }),
            layout: Some(LayoutMetric {
                score: 0.6,
                diff_regions: vec![LayoutDiffRegion {
                    x: 0.1,
                    y: 0.1,
                    width: 0.2,
                    height: 0.2,
                    kind: LayoutDiffKind::ExtraElement,
                    element_type: Some("button".to_string()),
                    label: None,
                }],
            }),
            typography: None,
            color: Some(ColorMetric {
                score: 0.5,
                diffs: vec![ColorDiff {
                    kind: ColorDiffKind::PrimaryColorShift,
                    ref_color: "#FFFFFF".to_string(),
                    impl_color: "#000000".to_string(),
                    delta_e: Some(10.0),
                }],
            }),
            content: Some(ContentMetric {
                score: 0.4,
                missing_text: vec!["Hero title".to_string()],
                extra_text: vec!["Extra banner".to_string()],
            }),
        };

        let ordered = generate_top_issues(&scores, 10);
        let missing_idx = ordered
            .iter()
            .position(|m| m.contains("missing in the implementation"))
            .expect("missing text issue");
        let primary_idx = ordered
            .iter()
            .position(|m| m.contains("Primary color"))
            .expect("primary color issue");
        let layout_idx = ordered
            .iter()
            .position(|m| m.contains("appears in implementation"))
            .expect("layout issue");
        let pixel_minor_idx = ordered
            .iter()
            .position(|m| m.contains("minor pixel difference"))
            .expect("pixel minor issue");
        let extra_text_idx = ordered
            .iter()
            .position(|m| m.contains("Extra text"))
            .expect("extra text issue");

        assert!(layout_idx > missing_idx);
        assert!(layout_idx > primary_idx);
        assert!(pixel_minor_idx > layout_idx);
        assert!(extra_text_idx > layout_idx);

        let top_three = generate_top_issues(&scores, 3);
        assert_eq!(top_three.len(), 3);
        assert!(top_three
            .iter()
            .all(|m| !m.contains("minor pixel difference") && !m.contains("Extra text")));
    }

    #[test]
    fn generate_top_issues_includes_color_and_typography_and_respects_limit() {
        let scores = MetricScores {
            pixel: None,
            layout: None,
            typography: Some(TypographyMetric {
                score: 0.6,
                diffs: vec![TypographyDiff {
                    element_id_ref: Some("title".into()),
                    element_id_impl: None,
                    issues: vec![TypographyIssue::FontFamilyMismatch],
                    details: None,
                }],
            }),
            color: Some(ColorMetric {
                score: 0.5,
                diffs: vec![ColorDiff {
                    kind: ColorDiffKind::AccentColorShift,
                    ref_color: "#FFFFFF".to_string(),
                    impl_color: "#111111".to_string(),
                    delta_e: Some(8.0),
                }],
            }),
            content: None,
        };

        let issues = generate_top_issues(&scores, 1);
        assert_eq!(issues.len(), 1, "limit should cap issue count to 1");
        assert!(
            issues[0].to_ascii_lowercase().contains("color")
                || issues[0].to_ascii_lowercase().contains("font"),
            "expected a color or typography issue first, got: {:?}",
            issues
        );

        let all_issues = generate_top_issues(&scores, 5);
        assert_eq!(
            all_issues.len(),
            2,
            "should include both color and typography"
        );
        assert!(
            all_issues
                .iter()
                .any(|m| m.to_ascii_lowercase().contains("font family")),
            "typography issue should be present: {:?}",
            all_issues
        );
        assert!(
            all_issues
                .iter()
                .any(|m| m.to_ascii_lowercase().contains("color shift")),
            "color issue should be present: {:?}",
            all_issues
        );
    }

    #[test]
    fn generate_top_issues_prioritizes_palette_over_typography() {
        let scores = MetricScores {
            pixel: None,
            layout: None,
            typography: Some(TypographyMetric {
                score: 0.7,
                diffs: vec![TypographyDiff {
                    element_id_ref: Some("caption".into()),
                    element_id_impl: Some("caption_impl".into()),
                    issues: vec![TypographyIssue::LineHeightDiff],
                    details: None,
                }],
            }),
            color: Some(ColorMetric {
                score: 0.6,
                diffs: vec![ColorDiff {
                    kind: ColorDiffKind::BackgroundColorShift,
                    ref_color: "#111111".to_string(),
                    impl_color: "#222222".to_string(),
                    delta_e: Some(4.0),
                }],
            }),
            content: None,
        };

        let issues = generate_top_issues(&scores, 5);
        let color_idx = issues
            .iter()
            .position(|m| m.to_ascii_lowercase().contains("color shift"))
            .expect("color issue present");
        let typo_idx = issues
            .iter()
            .position(|m| m.to_ascii_lowercase().contains("line height"))
            .expect("typography issue present");
        assert!(
            color_idx < typo_idx,
            "palette shifts should outrank minor typography issues"
        );
    }

    #[test]
    fn pixel_metric_identical_images_score_one() {
        let ref_img = solid_image([10, 20, 30, 255]);
        let impl_img = solid_image([10, 20, 30, 255]);
        let ref_view = view_from_file(ref_img.path(), 4, 4);
        let impl_view = view_from_file(impl_img.path(), 4, 4);
        let metric = PixelSimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Pixel(p) => p.score,
            _ => unreachable!(),
        };
        assert!((score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pixel_metric_completely_different_scores_low() {
        let ref_img = solid_image([0, 0, 0, 255]);
        let impl_img = solid_image([255, 255, 255, 255]);
        let ref_view = view_from_file(ref_img.path(), 4, 4);
        let impl_view = view_from_file(impl_img.path(), 4, 4);
        let metric = PixelSimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Pixel(p) => p.score,
            _ => unreachable!(),
        };
        assert!(score < 0.2);
    }

    #[test]
    fn pixel_metric_partial_difference_scores_between_zero_and_one() {
        let ref_img = solid_split_image(Rgba([0, 0, 0, 255]), Rgba([0, 0, 0, 255]));
        let impl_img = solid_split_image(Rgba([0, 0, 0, 255]), Rgba([255, 0, 0, 255]));
        let ref_view = view_from_image(&ref_img);
        let impl_view = view_from_image(&impl_img);
        let metric = PixelSimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Pixel(p) => p.score,
            _ => unreachable!(),
        };
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn layout_metric_partial_match_scores_between_zero_and_one() {
        let ref_view = view_with_dom(vec![
            ("button", bbox(0.0, 0.0, 0.5, 0.5)),
            ("img", bbox(0.6, 0.1, 0.3, 0.3)),
        ]);
        let impl_view = view_with_dom(vec![
            ("button", bbox(0.02, 0.02, 0.48, 0.48)),
            ("img", bbox(0.6, 0.1, 0.3, 0.3)),
            ("div", bbox(0.1, 0.8, 0.2, 0.1)),
        ]);
        let metric = LayoutSimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Layout(m) => m.score,
            _ => unreachable!(),
        };
        assert!(score > 0.5 && score < 1.0);
    }

    #[test]
    fn layout_metric_reports_extra_elements() {
        let ref_view = view_with_dom(vec![("button", bbox(0.0, 0.0, 0.5, 0.5))]);
        let impl_view = view_with_dom(vec![
            ("button", bbox(0.0, 0.0, 0.5, 0.5)),
            ("img", bbox(0.6, 0.1, 0.3, 0.3)),
        ]);
        let metric = LayoutSimilarity::default();
        let layout = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Layout(m) => m,
            _ => unreachable!(),
        };
        assert!(
            layout
                .diff_regions
                .iter()
                .any(|d| matches!(d.kind, LayoutDiffKind::ExtraElement)),
            "extra elements should be reported"
        );
        assert!(layout.score < 1.0);
    }

    #[test]
    fn layout_metric_missing_all_elements_scores_low() {
        let ref_view = view_with_dom(vec![
            ("button", bbox(0.0, 0.0, 0.5, 0.5)),
            ("img", bbox(0.6, 0.1, 0.3, 0.3)),
        ]);
        let impl_view = view_with_dom(vec![]);
        let metric = LayoutSimilarity::default();
        let layout = match metric.compute(&ref_view, &impl_view).expect("should score even when implementation layout is empty") {
            MetricResult::Layout(m) => m,
            _ => unreachable!(),
        };
        assert!(
            layout.score <= 0.05,
            "score should be very low when implementation is empty: {}",
            layout.score
        );
        assert_eq!(
            layout.diff_regions.len(),
            2,
            "should mark all reference elements as missing"
        );
        assert!(layout
            .diff_regions
            .iter()
            .all(|d| matches!(d.kind, LayoutDiffKind::MissingElement)));
    }

    #[test]
    fn layout_metric_identical_elements_scores_one() {
        let ref_view = view_with_dom(vec![
            ("button", bbox(0.0, 0.0, 0.5, 0.5)),
            ("img", bbox(0.4, 0.4, 0.2, 0.2)),
        ]);
        let impl_view = ref_view.clone();
        let metric = LayoutSimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Layout(m) => m.score,
            _ => unreachable!(),
        };
        assert!((score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn layout_metric_errors_when_reference_missing_layout() {
        let ref_view = dummy_view();
        let impl_view = view_with_dom(vec![("div", bbox(0.0, 0.0, 0.5, 0.5))]);
        let metric = LayoutSimilarity::default();
        let err = metric.compute(&ref_view, &impl_view).unwrap_err();
        let msg = format!("{err:?}").to_ascii_lowercase();
        assert!(
            msg.contains("reference"),
            "expected reference layout error, got {msg}"
        );
    }

    #[test]
    fn typography_metric_identical_text_scores_one() {
        let ref_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let impl_view = ref_view.clone();
        let metric = TypographySimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Typography(t) => t.score,
            _ => unreachable!(),
        };
        assert!((score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn typography_metric_weight_difference_penalized() {
        let ref_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let impl_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("700".into()),
                line_height: Some(24.0),
            },
        );
        let metric = TypographySimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Typography(t) => t.score,
            _ => unreachable!(),
        };
        assert!(score < 1.0);
    }

    #[test]
    fn typography_metric_font_family_mismatch_penalized() {
        let ref_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let impl_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Arial".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let metric = TypographySimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Typography(t) => t.score,
            _ => unreachable!(),
        };
        assert!(score < 1.0);
    }

    #[test]
    fn typography_metric_line_height_mismatch_penalized() {
        let ref_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let impl_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(18.0),
            },
        );
        let metric = TypographySimilarity::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Typography(t) => t.score,
            _ => unreachable!(),
        };
        assert!(score < 1.0);
    }

    #[test]
    fn typography_metric_small_size_difference_within_tolerance_scores_high() {
        let mut metric = TypographySimilarity::default();
        metric.size_tolerance = 0.2;
        let ref_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(16.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let impl_view = view_with_text(
            "Hello",
            TypographyStyle {
                font_family: Some("Inter".into()),
                font_size: Some(15.0),
                font_weight: Some("400".into()),
                line_height: Some(24.0),
            },
        );
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Typography(t) => t.score,
            _ => unreachable!(),
        };
        assert!(score > 0.8, "score should remain high for small size diff");
    }

    #[test]
    fn color_metric_identical_palettes_score_one() {
        let ref_img = solid_split_image(Rgba([10, 20, 30, 255]), Rgba([40, 50, 60, 255]));
        let impl_img = solid_split_image(Rgba([10, 20, 30, 255]), Rgba([40, 50, 60, 255]));
        let ref_view = view_from_image(&ref_img);
        let impl_view = view_from_image(&impl_img);
        let metric = ColorPaletteMetric::default();
        let score = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Color(c) => c.score,
            _ => unreachable!(),
        };
        assert!((score - 1.0).abs() < 1e-3);
    }

    #[test]
    fn color_metric_detects_palette_shift() {
        let ref_img = solid_split_image(Rgba([0, 0, 0, 255]), Rgba([255, 255, 255, 255]));
        let impl_img = solid_split_image(Rgba([0, 0, 0, 255]), Rgba([250, 0, 0, 255]));
        let ref_view = view_from_image(&ref_img);
        let impl_view = view_from_image(&impl_img);
        let metric = ColorPaletteMetric::default();

        let color = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Color(c) => c,
            _ => unreachable!(),
        };

        assert!(color.score < 0.9, "palette shift should reduce score");
        assert!(
            !color.diffs.is_empty(),
            "expected at least one color difference entry"
        );
        assert!(
            color
                .diffs
                .iter()
                .any(|d| d.ref_color != d.impl_color || d.delta_e.unwrap_or(0.0) > 1.0),
            "diff entries should carry ref/impl colors or delta"
        );
    }

    #[test]
    fn content_metric_missing_and_extra_text_affect_score() {
        let ref_view = view_with_dom(vec![("p:Hello", bbox(0.0, 0.0, 0.5, 0.5))]);
        let impl_view = view_with_dom(vec![
            ("p:Hello", bbox(0.0, 0.0, 0.5, 0.5)),
            ("h1:Extra", bbox(0.1, 0.1, 0.3, 0.3)),
        ]);
        let metric = ContentSimilarity::default();
        let result = metric.compute(&ref_view, &impl_view).unwrap();
        let content = match result {
            MetricResult::Content(c) => c,
            _ => unreachable!(),
        };
        assert!(content.score < 1.0);
        assert!(
            content.extra_text.iter().any(|t| t.contains("extra"))
                || !content.extra_text.is_empty()
        );
    }

    #[test]
    fn content_metric_extra_text_only_penalizes_score() {
        let ref_view = view_with_dom(vec![("p:Hello", bbox(0.0, 0.0, 0.5, 0.5))]);
        let impl_view = view_with_dom(vec![
            ("p:Hello", bbox(0.0, 0.0, 0.5, 0.5)),
            ("p:Extra", bbox(0.1, 0.1, 0.3, 0.2)),
        ]);
        let metric = ContentSimilarity::default();
        let content = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Content(c) => c,
            _ => unreachable!(),
        };
        assert!(content.score < 1.0);
        assert!(content.missing_text.is_empty());
        assert_eq!(content.extra_text.len(), 1);
    }

    #[test]
    fn content_metric_all_text_missing_penalizes_score() {
        let ref_view = view_with_dom(vec![
            ("p:Hello", bbox(0.0, 0.0, 0.5, 0.5)),
            ("h1:Title", bbox(0.0, 0.5, 0.5, 0.5)),
        ]);
        let impl_view = view_with_dom(vec![]);
        let metric = ContentSimilarity::default();
        let content = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Content(c) => c,
            _ => unreachable!(),
        };
        assert!(content.score < 0.2);
        assert_eq!(content.missing_text.len(), 2);
        assert!(content.extra_text.is_empty());
    }

    #[test]
    fn content_metric_no_text_returns_full_score() {
        let ref_view = dummy_view();
        let impl_view = dummy_view();
        let metric = ContentSimilarity::default();
        let content = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Content(c) => c,
            _ => unreachable!(),
        };
        assert!((content.score - 1.0).abs() < f32::EPSILON);
        assert!(content.missing_text.is_empty());
        assert!(content.extra_text.is_empty());
    }

    #[test]
    fn content_metric_completely_mismatched_text_penalizes_and_reports() {
        let ref_view = view_with_dom(vec![
            ("p:Alpha", bbox(0.0, 0.0, 0.5, 0.5)),
            ("h1:Beta", bbox(0.1, 0.1, 0.3, 0.2)),
        ]);
        let impl_view = view_with_dom(vec![
            ("p:Gamma", bbox(0.2, 0.2, 0.4, 0.3)),
            ("h2:Delta", bbox(0.3, 0.3, 0.2, 0.2)),
        ]);
        let metric = ContentSimilarity::default();
        let content = match metric.compute(&ref_view, &impl_view).unwrap() {
            MetricResult::Content(c) => c,
            _ => unreachable!(),
        };
        assert!(content.score < 0.5);
        assert_eq!(content.missing_text.len(), 2);
        assert_eq!(content.extra_text.len(), 2);
    }

    // Helpers for tests
    fn dummy_view() -> NormalizedView {
        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: "screenshot.png".into(),
            width: 100,
            height: 100,
            dom: None,
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    fn view_from_file(path: &std::path::Path, width: u32, height: u32) -> NormalizedView {
        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: path.to_path_buf(),
            width,
            height,
            dom: None,
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    fn solid_image(color: [u8; 4]) -> NamedTempFile {
        let mut img = RgbaImage::new(8, 8);
        for pixel in img.pixels_mut() {
            *pixel = Rgba(color);
        }
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        img.save_with_format(file.path(), image::ImageFormat::Png)
            .expect("write image");
        file
    }

    fn solid_split_image(left: Rgba<u8>, right: Rgba<u8>) -> NamedTempFile {
        let mut img = RgbaImage::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                let px = if x < 2 { left } else { right };
                img.put_pixel(x, y, px);
            }
        }
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        img.save_with_format(file.path(), ImageFormat::Png)
            .expect("write split image");
        file
    }

    fn bbox(x: f32, y: f32, width: f32, height: f32) -> crate::types::BoundingBox {
        crate::types::BoundingBox {
            x,
            y,
            width,
            height,
        }
    }

    fn view_with_dom(nodes: Vec<(&str, crate::types::BoundingBox)>) -> NormalizedView {
        use crate::types::{DomNode, DomSnapshot};
        let dom_nodes = nodes
            .into_iter()
            .enumerate()
            .map(|(idx, (spec, bbox))| {
                let mut parts = spec.splitn(2, ':');
                let tag = parts.next().unwrap_or("div").to_string();
                let text = parts.next().map(|t| t.to_string());
                DomNode {
                    id: format!("n{}", idx),
                    tag,
                    children: vec![],
                    parent: None,
                    attributes: std::collections::HashMap::new(),
                    text,
                    bounding_box: bbox,
                    computed_style: None,
                }
            })
            .collect();

        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: "dummy.png".into(),
            width: 100,
            height: 100,
            dom: Some(DomSnapshot {
                url: None,
                title: None,
                nodes: dom_nodes,
            }),
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    fn view_with_text(text: &str, style: TypographyStyle) -> NormalizedView {
        use crate::types::{DomNode, DomSnapshot};
        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: "dummy.png".into(),
            width: 100,
            height: 100,
            dom: Some(DomSnapshot {
                url: None,
                title: None,
                nodes: vec![DomNode {
                    id: "t1".into(),
                    tag: "p".into(),
                    children: vec![],
                    parent: None,
                    attributes: std::collections::HashMap::new(),
                    text: Some(text.to_string()),
                    bounding_box: bbox(0.0, 0.0, 0.5, 0.1),
                    computed_style: Some(ComputedStyle {
                        font_family: style.font_family.clone(),
                        font_size: style.font_size,
                        font_weight: style.font_weight.clone(),
                        line_height: style.line_height,
                        color: None,
                        background_color: None,
                        display: None,
                        visibility: None,
                        opacity: None,
                    }),
                }],
            }),
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    fn view_from_image(file: &NamedTempFile) -> NormalizedView {
        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: file.path().to_path_buf(),
            width: 4,
            height: 2,
            dom: None,
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    struct StubMetric {
        kind: MetricKind,
        score: f32,
        calls: Rc<RefCell<u32>>,
    }

    impl StubMetric {
        fn pixel(score: f32, calls: Rc<RefCell<u32>>) -> Self {
            Self {
                kind: MetricKind::Pixel,
                score,
                calls,
            }
        }

        fn layout(score: f32, calls: Rc<RefCell<u32>>) -> Self {
            Self {
                kind: MetricKind::Layout,
                score,
                calls,
            }
        }

        fn result(&self) -> MetricResult {
            match self.kind {
                MetricKind::Pixel => MetricResult::Pixel(PixelMetric {
                    score: self.score,
                    diff_regions: vec![],
                }),
                MetricKind::Layout => MetricResult::Layout(LayoutMetric {
                    score: self.score,
                    diff_regions: vec![],
                }),
                MetricKind::Typography => MetricResult::Typography(TypographyMetric {
                    score: self.score,
                    diffs: vec![],
                }),
                MetricKind::Color => MetricResult::Color(ColorMetric {
                    score: self.score,
                    diffs: vec![],
                }),
                MetricKind::Content => MetricResult::Content(ContentMetric {
                    score: self.score,
                    missing_text: vec![],
                    extra_text: vec![],
                }),
            }
        }
    }

    impl Metric for StubMetric {
        fn kind(&self) -> MetricKind {
            self.kind
        }

        fn compute(
            &self,
            _reference: &NormalizedView,
            _implementation: &NormalizedView,
        ) -> Result<MetricResult> {
            *self.calls.borrow_mut() += 1;
            Ok(self.result())
        }
    }

    #[test]
    fn cluster_diff_regions_empty_on_no_diffs() {
        let diff_map = vec![0.0; 64];
        let thresholds = PixelDiffThresholds::default();
        let regions = cluster_diff_regions(&diff_map, 8, 8, 4, &thresholds);
        assert!(regions.is_empty());
    }

    #[test]
    fn cluster_diff_regions_detects_minor_block() {
        let mut diff_map = vec![0.0; 64];
        for row in 0..4 {
            for col in 0..4 {
                diff_map[row * 8 + col] = 0.08;
            }
        }
        let thresholds = PixelDiffThresholds::default();
        let regions = cluster_diff_regions(&diff_map, 8, 8, 4, &thresholds);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].severity, DiffSeverity::Minor);
        assert!((regions[0].x - 0.0).abs() < f32::EPSILON);
        assert!((regions[0].y - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cluster_diff_regions_detects_major_block() {
        let mut diff_map = vec![0.0; 64];
        for row in 0..4 {
            for col in 4..8 {
                diff_map[row * 8 + col] = 0.5;
            }
        }
        let thresholds = PixelDiffThresholds::default();
        let regions = cluster_diff_regions(&diff_map, 8, 8, 4, &thresholds);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].severity, DiffSeverity::Major);
    }

    #[test]
    fn cluster_diff_regions_handles_multiple_blocks() {
        let mut diff_map = vec![0.0; 64];
        for row in 0..4 {
            for col in 0..4 {
                diff_map[row * 8 + col] = 0.35;
            }
        }
        for row in 4..8 {
            for col in 4..8 {
                diff_map[row * 8 + col] = 0.06;
            }
        }
        let thresholds = PixelDiffThresholds::default();
        let regions = cluster_diff_regions(&diff_map, 8, 8, 4, &thresholds);
        assert_eq!(regions.len(), 2);
        let severities: Vec<_> = regions.iter().map(|r| r.severity).collect();
        assert!(severities.contains(&DiffSeverity::Major));
        assert!(severities.contains(&DiffSeverity::Minor));
    }

    #[test]
    fn cluster_diff_regions_returns_empty_for_zero_dimensions() {
        let diff_map = vec![0.5; 64];
        let thresholds = PixelDiffThresholds::default();
        assert!(cluster_diff_regions(&diff_map, 0, 8, 4, &thresholds).is_empty());
        assert!(cluster_diff_regions(&diff_map, 8, 0, 4, &thresholds).is_empty());
        assert!(cluster_diff_regions(&diff_map, 8, 8, 0, &thresholds).is_empty());
        assert!(cluster_diff_regions(&[], 8, 8, 4, &thresholds).is_empty());
    }
}
