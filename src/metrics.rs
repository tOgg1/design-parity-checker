use crate::error::DpcError;
use crate::types::{
    ColorDiff, ColorDiffKind, ColorMetric, ContentMetric, DiffSeverity, LayoutDiffKind,
    LayoutDiffRegion, LayoutMetric, MetricScores, NormalizedView, PixelDiffReason, PixelDiffRegion,
    PixelMetric, TypographyDiff, TypographyIssue, TypographyMetric,
};
use crate::Result;
use image::{DynamicImage, GenericImageView};
use palette::{convert::FromColorUnclamped, Lab, Srgb};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

mod pixel;
pub use pixel::{cluster_diff_regions, PixelDiffThresholds, PixelSimilarity};

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

pub trait Metric {
    fn kind(&self) -> MetricKind;
    fn compute(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<MetricResult>;
}

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

#[derive(Debug, Clone, Copy)]
pub struct LayoutSimilarity {
    pub iou_threshold: f32,
    pub match_threshold: f32,
}

impl Default for LayoutSimilarity {
    fn default() -> Self {
        Self {
            iou_threshold: 0.5,
            match_threshold: 0.1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LayoutElement {
    kind: ElementKind,
    bbox: crate::types::BoundingBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Button,
    Heading,
    Text,
    Image,
    Input,
    Other,
}

impl LayoutSimilarity {
    fn extract_elements(view: &NormalizedView) -> Option<Vec<LayoutElement>> {
        if let Some(dom) = &view.dom {
            let elements = dom
                .nodes
                .iter()
                .map(|node| LayoutElement {
                    kind: element_kind_from_dom(node),
                    bbox: node.bounding_box,
                })
                .collect::<Vec<_>>();
            if !elements.is_empty() {
                return Some(elements);
            }
        }

        if let Some(figma) = &view.figma_tree {
            let elements = figma
                .nodes
                .iter()
                .map(|node| LayoutElement {
                    kind: element_kind_from_figma(node),
                    bbox: node.bounding_box,
                })
                .collect::<Vec<_>>();
            if !elements.is_empty() {
                return Some(elements);
            }
        }

        None
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
        let ref_elements = LayoutSimilarity::extract_elements(reference).ok_or_else(|| {
            DpcError::Config("No layout elements available in reference view".to_string())
        })?;
        let mut impl_elements =
            LayoutSimilarity::extract_elements(implementation).ok_or_else(|| {
                DpcError::Config("No layout elements available in implementation view".to_string())
            })?;

        let ref_count = ref_elements.len();
        let impl_count = impl_elements.len();

        let mut matches = Vec::new();

        for ref_el in &ref_elements {
            if let Some((idx, iou)) = best_match(
                ref_el,
                &impl_elements,
                self.match_threshold,
                self.iou_threshold,
            ) {
                matches.push((ref_el, impl_elements[idx], iou));
                impl_elements.remove(idx);
            }
        }

        let matched = matches.len() as f32;
        let max_count = ref_count.max(impl_count) as f32;
        let match_rate = if max_count == 0.0 {
            1.0
        } else {
            matched / max_count
        };

        let avg_iou = if matches.is_empty() {
            0.0
        } else {
            matches.iter().map(|(_, _, iou)| *iou).sum::<f32>() / matches.len() as f32
        };

        let score = 0.5 * match_rate + 0.5 * avg_iou;

        let mut diff_regions = Vec::new();

        for ref_el in &ref_elements {
            if !matches.iter().any(|(r, _, _)| std::ptr::eq(*r, ref_el)) {
                diff_regions.push(LayoutDiffRegion {
                    x: ref_el.bbox.x,
                    y: ref_el.bbox.y,
                    width: ref_el.bbox.width,
                    height: ref_el.bbox.height,
                    kind: LayoutDiffKind::MissingElement,
                    element_type: Some(ref_el.kind.as_str().to_string()),
                    label: None,
                });
            }
        }

        for extra in &impl_elements {
            diff_regions.push(LayoutDiffRegion {
                x: extra.bbox.x,
                y: extra.bbox.y,
                width: extra.bbox.width,
                height: extra.bbox.height,
                kind: LayoutDiffKind::ExtraElement,
                element_type: Some(extra.kind.as_str().to_string()),
                label: None,
            });
        }

        for (_, impl_el, iou) in &matches {
            if *iou < self.iou_threshold {
                diff_regions.push(LayoutDiffRegion {
                    x: impl_el.bbox.x,
                    y: impl_el.bbox.y,
                    width: impl_el.bbox.width,
                    height: impl_el.bbox.height,
                    kind: LayoutDiffKind::PositionShift,
                    element_type: Some(impl_el.kind.as_str().to_string()),
                    label: None,
                });
            }
        }

        for impl_el in &impl_elements {
            diff_regions.push(LayoutDiffRegion {
                x: impl_el.bbox.x,
                y: impl_el.bbox.y,
                width: impl_el.bbox.width,
                height: impl_el.bbox.height,
                kind: LayoutDiffKind::ExtraElement,
                element_type: Some(impl_el.kind.as_str().to_string()),
                label: None,
            });
        }

        Ok(MetricResult::Layout(LayoutMetric {
            score,
            diff_regions,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TypographySimilarity {
    pub size_tolerance: f32,
    pub line_height_tolerance: f32,
}

impl Default for TypographySimilarity {
    fn default() -> Self {
        Self {
            size_tolerance: 0.1,
            line_height_tolerance: 0.1,
        }
    }
}

#[derive(Debug, Clone)]
struct TypographyElement {
    id: String,
    text: String,
    family: Option<String>,
    size: Option<f32>,
    weight: Option<String>,
    line_height: Option<f32>,
}

impl TypographySimilarity {
    fn extract(view: &NormalizedView) -> Option<Vec<TypographyElement>> {
        if let Some(dom) = &view.dom {
            let mut elems = Vec::new();
            for node in &dom.nodes {
                if let Some(text) = &node.text {
                    if let Some(style) = &node.computed_style {
                        elems.push(TypographyElement {
                            id: node.id.clone(),
                            text: text.clone(),
                            family: style.font_family.clone(),
                            size: style.font_size,
                            weight: style.font_weight.clone(),
                            line_height: style.line_height,
                        });
                    }
                }
            }
            if !elems.is_empty() {
                return Some(elems);
            }
        }

        if let Some(figma) = &view.figma_tree {
            let mut elems = Vec::new();
            for node in &figma.nodes {
                if let (Some(text), Some(style)) = (&node.text, &node.typography) {
                    elems.push(TypographyElement {
                        id: node.id.clone(),
                        text: text.clone(),
                        family: style.font_family.clone(),
                        size: style.font_size,
                        weight: style.font_weight.clone(),
                        line_height: style.line_height,
                    });
                }
            }
            if !elems.is_empty() {
                return Some(elems);
            }
        }

        None
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
        let ref_elems = TypographySimilarity::extract(reference).ok_or_else(|| {
            DpcError::Config("No typography elements available in reference view".to_string())
        })?;
        let impl_elems = TypographySimilarity::extract(implementation).ok_or_else(|| {
            DpcError::Config("No typography elements available in implementation view".to_string())
        })?;

        let mut impl_by_text: HashMap<String, Vec<TypographyElement>> = HashMap::new();
        for el in impl_elems {
            if let Some(norm) = normalize_label(&el.text) {
                impl_by_text.entry(norm).or_default().push(el);
            }
        }

        let mut total_penalty = 0.0f32;
        let mut comparisons = 0usize;
        let mut diffs: Vec<TypographyDiff> = Vec::new();

        for ref_el in &ref_elems {
            comparisons += 1;
            let Some(norm_text) = normalize_label(&ref_el.text) else {
                continue;
            };

            let maybe_impl_list = impl_by_text.get_mut(&norm_text);
            if let Some(list) = maybe_impl_list {
                if let Some(impl_el) = list.pop() {
                    let (penalty, issues) = typography_penalty(
                        ref_el,
                        &impl_el,
                        self.size_tolerance,
                        self.line_height_tolerance,
                    );
                    total_penalty += penalty;
                    if !issues.is_empty() {
                        diffs.push(TypographyDiff {
                            element_id_ref: Some(ref_el.id.clone()),
                            element_id_impl: Some(impl_el.id.clone()),
                            issues,
                            details: None,
                        });
                    }
                } else {
                    total_penalty += 1.0;
                    diffs.push(TypographyDiff {
                        element_id_ref: Some(ref_el.id.clone()),
                        element_id_impl: None,
                        issues: vec![TypographyIssue::FontFamilyMismatch],
                        details: None,
                    });
                }
                if list.is_empty() {
                    impl_by_text.remove(&norm_text);
                }
            } else {
                total_penalty += 1.0;
                diffs.push(TypographyDiff {
                    element_id_ref: Some(ref_el.id.clone()),
                    element_id_impl: None,
                    issues: vec![TypographyIssue::FontFamilyMismatch],
                    details: None,
                });
            }
        }

        // penalize extra implementation texts that did not match any reference
        for list in impl_by_text.values() {
            for impl_el in list {
                comparisons += 1;
                total_penalty += 0.2;
                diffs.push(TypographyDiff {
                    element_id_ref: None,
                    element_id_impl: Some(impl_el.id.clone()),
                    issues: vec![TypographyIssue::FontFamilyMismatch],
                    details: None,
                });
            }
        }

        let score = if comparisons == 0 {
            1.0
        } else {
            (1.0 - (total_penalty / comparisons as f32)).clamp(0.0, 1.0)
        };

        Ok(MetricResult::Typography(TypographyMetric { score, diffs }))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ColorPaletteMetric {
    pub clusters: usize,
    pub sample_stride: u32,
}

impl Default for ColorPaletteMetric {
    fn default() -> Self {
        Self {
            clusters: 5,
            sample_stride: 4,
        }
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
        let ref_img = image::open(&reference.screenshot_path).map_err(DpcError::from)?;
        let impl_img = image::open(&implementation.screenshot_path).map_err(DpcError::from)?;

        let ref_palette = dominant_palette(&ref_img, self.clusters, self.sample_stride);
        let impl_palette = dominant_palette(&impl_img, self.clusters, self.sample_stride);

        let mut diffs = palette_diffs(&ref_palette, &impl_palette, 3);
        let mut score = palette_similarity(&ref_palette, &impl_palette);
        let needs_fallback = diffs.is_empty()
            || diffs
                .iter()
                .all(|d| d.ref_color == d.impl_color && d.delta_e.unwrap_or(0.0) <= 1.0);

        if needs_fallback {
            let avg_ref = average_rgb(&ref_img);
            let avg_impl = average_rgb(&impl_img);
            let delta = rgb_distance(&avg_ref, &avg_impl);
            diffs.push(ColorDiff {
                kind: ColorDiffKind::PrimaryColorShift,
                ref_color: format!("#{:02X}{:02X}{:02X}", avg_ref[0], avg_ref[1], avg_ref[2]),
                impl_color: format!("#{:02X}{:02X}{:02X}", avg_impl[0], avg_impl[1], avg_impl[2]),
                delta_e: Some(delta),
            });
        }

        let has_meaningful_diff = diffs
            .iter()
            .any(|d| d.ref_color != d.impl_color || d.delta_e.unwrap_or(0.0) > 1.0);
        if has_meaningful_diff {
            score = score.min(0.8);
        }

        Ok(MetricResult::Color(ColorMetric { score, diffs }))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContentSimilarity {
    pub match_threshold: f32,
    pub extra_penalty_weight: f32,
}

impl Default for ContentSimilarity {
    fn default() -> Self {
        Self {
            match_threshold: 0.7,
            extra_penalty_weight: 0.2,
        }
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
        let ref_texts = extract_texts(reference);
        let impl_texts = extract_texts(implementation);

        if ref_texts.is_empty() && impl_texts.is_empty() {
            return Ok(MetricResult::Content(ContentMetric {
                score: 1.0,
                missing_text: vec![],
                extra_text: vec![],
            }));
        }

        let normalized_ref: Vec<(String, String)> = ref_texts
            .into_iter()
            .filter_map(|original| normalize_text(&original).map(|norm| (original, norm)))
            .collect();
        let normalized_impl: Vec<(String, String)> = impl_texts
            .into_iter()
            .filter_map(|original| normalize_text(&original).map(|norm| (original, norm)))
            .collect();

        if normalized_ref.is_empty() && normalized_impl.is_empty() {
            return Ok(MetricResult::Content(ContentMetric {
                score: 1.0,
                missing_text: vec![],
                extra_text: vec![],
            }));
        }

        let mut matched_impl = vec![false; normalized_impl.len()];
        let mut matched_count = 0usize;
        let mut missing_text = Vec::new();

        for (ref_orig, ref_norm) in &normalized_ref {
            let mut best_score = 0.0f32;
            let mut best_idx = None;

            for (idx, (_impl_orig, impl_norm)) in normalized_impl.iter().enumerate() {
                let score = token_similarity(ref_norm, impl_norm);
                if score > best_score {
                    best_score = score;
                    best_idx = Some(idx);
                }
            }

            if best_score >= self.match_threshold {
                matched_count += 1;
                if let Some(idx) = best_idx {
                    matched_impl[idx] = true;
                }
            } else {
                missing_text.push(ref_orig.clone());
            }
        }

        let extra_text: Vec<String> = normalized_impl
            .iter()
            .enumerate()
            .filter_map(|(idx, (orig, _))| {
                if matched_impl[idx] {
                    None
                } else {
                    Some(orig.clone())
                }
            })
            .collect();

        let ref_len = normalized_ref.len() as f32;
        let base_score = if ref_len == 0.0 {
            1.0
        } else {
            matched_count as f32 / ref_len
        };

        let ref_chars: usize = normalized_ref.iter().map(|(orig, _)| orig.len()).sum();
        let extra_chars: usize = extra_text.iter().map(|s| s.len()).sum();
        let penalty = if ref_chars == 0 {
            0.0
        } else {
            let frac = extra_chars as f32 / ref_chars as f32;
            (frac * self.extra_penalty_weight).min(0.5)
        };

        let score = (base_score - penalty).clamp(0.0, 1.0);

        Ok(MetricResult::Content(ContentMetric {
            score,
            missing_text,
            extra_text,
        }))
    }
}

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

    let layout_available = has_layout_data(reference) && has_layout_data(implementation);
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

#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    pub pixel: f32,
    pub layout: f32,
    pub typography: f32,
    pub color: f32,
    pub content: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            pixel: 0.35,
            layout: 0.25,
            typography: 0.15,
            color: 0.15,
            content: 0.10,
        }
    }
}

impl ScoreWeights {
    pub fn sum(&self) -> f32 {
        self.pixel + self.layout + self.typography + self.color + self.content
    }
}

pub fn calculate_combined_score(scores: &MetricScores, weights: &ScoreWeights) -> f32 {
    let mut total_weight = 0.0f32;
    let mut weighted_sum = 0.0f32;

    if let Some(ref m) = scores.pixel {
        weighted_sum += weights.pixel * m.score;
        total_weight += weights.pixel;
    }

    if let Some(ref m) = scores.layout {
        weighted_sum += weights.layout * m.score;
        total_weight += weights.layout;
    }

    if let Some(ref m) = scores.typography {
        weighted_sum += weights.typography * m.score;
        total_weight += weights.typography;
    }

    if let Some(ref m) = scores.color {
        weighted_sum += weights.color * m.score;
        total_weight += weights.color;
    }

    if let Some(ref m) = scores.content {
        weighted_sum += weights.content * m.score;
        total_weight += weights.content;
    }

    if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    }
}

#[derive(Debug, Clone)]
struct RankedIssue {
    severity_rank: u8,
    message: String,
}

impl RankedIssue {
    fn major(message: impl Into<String>) -> Self {
        Self {
            severity_rank: 0,
            message: message.into(),
        }
    }

    fn moderate(message: impl Into<String>) -> Self {
        Self {
            severity_rank: 1,
            message: message.into(),
        }
    }

    fn minor(message: impl Into<String>) -> Self {
        Self {
            severity_rank: 2,
            message: message.into(),
        }
    }
}

pub fn generate_top_issues(scores: &MetricScores, max_issues: usize) -> Vec<String> {
    let mut issues: Vec<RankedIssue> = Vec::new();

    if let Some(ref pixel) = scores.pixel {
        issues.extend(issues_from_pixel(pixel));
    }

    if let Some(ref layout) = scores.layout {
        issues.extend(issues_from_layout(layout));
    }

    if let Some(ref typography) = scores.typography {
        issues.extend(issues_from_typography(typography));
    }

    if let Some(ref color) = scores.color {
        issues.extend(issues_from_color(color));
    }

    if let Some(ref content) = scores.content {
        issues.extend(issues_from_content(content));
    }

    issues.sort_by(|a, b| {
        a.severity_rank
            .cmp(&b.severity_rank)
            .then_with(|| a.message.cmp(&b.message))
    });
    issues
        .into_iter()
        .take(max_issues)
        .map(|i| i.message)
        .collect()
}

fn issues_from_pixel(metric: &PixelMetric) -> Vec<RankedIssue> {
    let mut issues = Vec::new();

    let major_count = metric
        .diff_regions
        .iter()
        .filter(|r| r.severity == DiffSeverity::Major)
        .count();
    let moderate_count = metric
        .diff_regions
        .iter()
        .filter(|r| r.severity == DiffSeverity::Moderate)
        .count();
    let minor_count = metric
        .diff_regions
        .iter()
        .filter(|r| r.severity == DiffSeverity::Minor)
        .count();

    if major_count > 0 {
        issues.push(RankedIssue::major(format!(
            "{} major pixel difference region{} detected.",
            major_count,
            if major_count == 1 { "" } else { "s" }
        )));
    }
    if moderate_count > 0 {
        issues.push(RankedIssue::moderate(format!(
            "{} moderate pixel difference region{} detected.",
            moderate_count,
            if moderate_count == 1 { "" } else { "s" }
        )));
    }
    if minor_count > 0 {
        issues.push(RankedIssue::minor(format!(
            "{} minor pixel difference region{} detected.",
            minor_count,
            if minor_count == 1 { "" } else { "s" }
        )));
    }

    issues
}

fn issues_from_layout(metric: &LayoutMetric) -> Vec<RankedIssue> {
    let mut issues = Vec::new();

    for region in &metric.diff_regions {
        let element_desc = region
            .label
            .as_ref()
            .map(|l| format!("'{}'", l))
            .or_else(|| region.element_type.clone())
            .unwrap_or_else(|| "element".to_string());

        let msg = match region.kind {
            LayoutDiffKind::MissingElement => {
                format!("{} is missing in the implementation.", element_desc)
            }
            LayoutDiffKind::ExtraElement => {
                format!(
                    "{} appears in implementation but not in reference.",
                    element_desc
                )
            }
            LayoutDiffKind::PositionShift => {
                format!("{} is shifted from its expected position.", element_desc)
            }
            LayoutDiffKind::SizeChange => {
                format!("{} has a different size than the reference.", element_desc)
            }
        };

        let ranked = match region.kind {
            LayoutDiffKind::MissingElement => RankedIssue::major(msg),
            LayoutDiffKind::ExtraElement => RankedIssue::moderate(msg),
            LayoutDiffKind::PositionShift | LayoutDiffKind::SizeChange => {
                RankedIssue::moderate(msg)
            }
        };
        issues.push(ranked);
    }

    issues
}

fn issues_from_typography(metric: &TypographyMetric) -> Vec<RankedIssue> {
    let mut issues = Vec::new();

    for diff in &metric.diffs {
        if diff.issues.is_empty() {
            continue;
        }

        let element_id = diff
            .element_id_ref
            .as_ref()
            .or(diff.element_id_impl.as_ref())
            .cloned()
            .unwrap_or_else(|| "text element".to_string());

        let issue_names: Vec<&str> = diff
            .issues
            .iter()
            .map(|i| match i {
                TypographyIssue::FontFamilyMismatch => "font family",
                TypographyIssue::FontSizeDiff => "font size",
                TypographyIssue::FontWeightDiff => "font weight",
                TypographyIssue::LineHeightDiff => "line height",
            })
            .collect();

        let msg = if issue_names.len() == 1 {
            format!(
                "{} has a different {} than the design.",
                element_id, issue_names[0]
            )
        } else {
            format!(
                "{} has different {} than the design.",
                element_id,
                issue_names.join(", ")
            )
        };

        let ranked = if diff.issues.contains(&TypographyIssue::FontFamilyMismatch) {
            RankedIssue::major(msg)
        } else if diff.issues.contains(&TypographyIssue::FontSizeDiff)
            || diff.issues.contains(&TypographyIssue::FontWeightDiff)
        {
            RankedIssue::moderate(msg)
        } else {
            RankedIssue::minor(msg)
        };

        issues.push(ranked);
    }

    issues
}

fn issues_from_color(metric: &ColorMetric) -> Vec<RankedIssue> {
    let mut issues = Vec::new();

    for diff in &metric.diffs {
        let kind_desc = match diff.kind {
            ColorDiffKind::PrimaryColorShift => "Primary color shift",
            ColorDiffKind::AccentColorShift => "Accent color shift",
            ColorDiffKind::BackgroundColorShift => "Background color shift",
        };

        let msg = format!(
            "{} differs: expected {}, got {}.",
            kind_desc, diff.ref_color, diff.impl_color
        );

        let ranked = match diff.kind {
            ColorDiffKind::PrimaryColorShift => RankedIssue::major(msg),
            ColorDiffKind::AccentColorShift => RankedIssue::major(msg),
            ColorDiffKind::BackgroundColorShift => RankedIssue::minor(msg),
        };
        issues.push(ranked);
    }

    issues
}

fn issues_from_content(metric: &ContentMetric) -> Vec<RankedIssue> {
    let mut issues = Vec::new();

    if !metric.missing_text.is_empty() {
        let count = metric.missing_text.len();
        if count <= 3 {
            for text in &metric.missing_text {
                let truncated = if text.len() > 50 {
                    format!("{}...", &text[..47])
                } else {
                    text.clone()
                };
                issues.push(RankedIssue::major(format!(
                    "Text '{}' is missing in the implementation.",
                    truncated
                )));
            }
        } else {
            issues.push(RankedIssue::major(format!(
                "{} text elements are missing in the implementation.",
                count
            )));
        }
    }

    if !metric.extra_text.is_empty() {
        let count = metric.extra_text.len();
        if count <= 3 {
            for text in &metric.extra_text {
                let truncated = if text.len() > 50 {
                    format!("{}...", &text[..47])
                } else {
                    text.clone()
                };
                issues.push(RankedIssue::minor(format!(
                    "Extra text '{}' appears in implementation but not in design.",
                    truncated
                )));
            }
        } else {
            issues.push(RankedIssue::minor(format!(
                "{} extra text elements appear in implementation but not in design.",
                count
            )));
        }
    }

    issues
}


fn element_kind_from_dom(node: &crate::types::DomNode) -> ElementKind {
    let tag = node.tag.to_ascii_lowercase();
    match tag.as_str() {
        "button" => ElementKind::Button,
        "img" => ElementKind::Image,
        "input" | "textarea" | "select" => ElementKind::Input,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => ElementKind::Heading,
        "p" | "span" | "div" => ElementKind::Text,
        _ => ElementKind::Other,
    }
}

fn element_kind_from_figma(node: &crate::types::FigmaNode) -> ElementKind {
    let kind = node.node_type.to_ascii_lowercase();
    match kind.as_str() {
        "text" => ElementKind::Text,
        "rectangle" | "ellipse" | "frame" | "component" => ElementKind::Other,
        "image" => ElementKind::Image,
        _ => ElementKind::Other,
    }
}

impl ElementKind {
    fn as_str(&self) -> &'static str {
        match self {
            ElementKind::Button => "button",
            ElementKind::Heading => "heading",
            ElementKind::Text => "text",
            ElementKind::Image => "image",
            ElementKind::Input => "input",
            ElementKind::Other => "other",
        }
    }
}

fn iou(a: &crate::types::BoundingBox, b: &crate::types::BoundingBox) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }

    let inter = (ix2 - ix1) * (iy2 - iy1);
    let area_a = a.width * a.height;
    let area_b = b.width * b.height;
    let union = area_a + area_b - inter;

    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn best_match(
    target: &LayoutElement,
    candidates: &[LayoutElement],
    match_threshold: f32,
    iou_threshold: f32,
) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (idx, cand) in candidates.iter().enumerate() {
        if cand.kind != target.kind {
            continue;
        }
        let overlap = iou(&target.bbox, &cand.bbox);
        if overlap < match_threshold {
            continue;
        }
        if best.is_none_or(|(_, score)| overlap > score) {
            best = Some((idx, overlap));
        }
    }

    best.filter(|(_, score)| *score >= iou_threshold)
}

fn typography_penalty(
    reference: &TypographyElement,
    implementation: &TypographyElement,
    size_tolerance: f32,
    line_height_tolerance: f32,
) -> (f32, Vec<TypographyIssue>) {
    const FAMILY_WEIGHT: f32 = 0.6;
    const SIZE_WEIGHT: f32 = 0.2;
    const WEIGHT_WEIGHT: f32 = 0.15;
    const LINE_WEIGHT: f32 = 0.05;

    let mut penalty = 0.0f32;
    let mut issues = Vec::new();

    let ref_family = canonical_family(reference.family.as_deref());
    let impl_family = canonical_family(implementation.family.as_deref());
    if ref_family != impl_family {
        penalty += FAMILY_WEIGHT;
        issues.push(TypographyIssue::FontFamilyMismatch);
    }

    if let (Some(ref_size), Some(impl_size)) = (reference.size, implementation.size) {
        if ref_size > 0.0 {
            let diff = ((impl_size - ref_size) / ref_size).abs();
            if diff > size_tolerance {
                penalty += SIZE_WEIGHT * diff.min(1.0);
                issues.push(TypographyIssue::FontSizeDiff);
            }
        }
    }

    let ref_weight = font_weight_category(reference.weight.as_deref());
    let impl_weight = font_weight_category(implementation.weight.as_deref());
    if ref_weight.is_some() && impl_weight.is_some() && ref_weight != impl_weight {
        penalty += WEIGHT_WEIGHT;
        issues.push(TypographyIssue::FontWeightDiff);
    }

    if let (Some(ref_lh), Some(impl_lh)) = (reference.line_height, implementation.line_height) {
        if ref_lh > 0.0 {
            let diff = ((impl_lh - ref_lh) / ref_lh).abs();
            if diff > line_height_tolerance {
                penalty += LINE_WEIGHT * diff.min(1.0);
                issues.push(TypographyIssue::LineHeightDiff);
            }
        }
    }

    (penalty, issues)
}

fn normalize_text(input: &str) -> Option<String> {
    let lower = input.to_lowercase();
    let mut cleaned = String::new();

    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            cleaned.push(ch);
        } else if ch.is_whitespace() {
            cleaned.push(' ');
        }
    }

    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn normalize_label(input: &str) -> Option<String> {
    normalize_text(input)
}

fn canonical_family(family: Option<&str>) -> String {
    let Some(fam) = family else {
        return "unknown".to_string();
    };
    let lower = fam.to_ascii_lowercase();
    if lower.contains("inter") {
        "inter".to_string()
    } else if lower.contains("roboto") {
        "roboto".to_string()
    } else if lower.contains("helvetica") {
        "helvetica".to_string()
    } else if lower.contains("arial") {
        "arial".to_string()
    } else if lower.contains("times") {
        "times".to_string()
    } else if lower.contains("georgia") {
        "georgia".to_string()
    } else {
        lower
    }
}

fn font_weight_category(weight: Option<&str>) -> Option<u16> {
    let w = weight?;
    let lower = w.trim().to_ascii_lowercase();
    if let Ok(num) = lower.parse::<u16>() {
        return Some(num);
    }
    match lower.as_str() {
        "thin" => Some(100),
        "extralight" | "ultralight" => Some(200),
        "light" => Some(300),
        "normal" | "regular" => Some(400),
        "medium" => Some(500),
        "semibold" | "demibold" => Some(600),
        "bold" => Some(700),
        "extrabold" | "ultrabold" => Some(800),
        "black" | "heavy" => Some(900),
        _ => None,
    }
}

fn dominant_palette(img: &DynamicImage, clusters: usize, stride: u32) -> Vec<(Lab, f32)> {
    let samples = sample_pixels(img, stride);
    if samples.is_empty() {
        return Vec::new();
    }

    let k = clusters.max(1).min(samples.len());
    kmeans(&samples, k, 8)
}

fn sample_pixels(img: &DynamicImage, stride: u32) -> Vec<(Lab, f32)> {
    let (w, h) = img.dimensions();
    let mut samples = Vec::new();
    let step = stride.max(1);

    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let pixel = img.get_pixel(x, y).0;
            let srgb = Srgb::new(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            );
            let lab: Lab = Lab::from_color_unclamped(srgb);
            samples.push((lab, 1.0));
        }
    }

    samples
}

fn kmeans(samples: &[(Lab, f32)], k: usize, iterations: usize) -> Vec<(Lab, f32)> {
    let mut centers: Vec<Lab> = Vec::with_capacity(k);
    let mut weights = Vec::with_capacity(k);

    let step = (samples.len() / k).max(1);
    for i in 0..k {
        centers.push(samples[i * step % samples.len()].0);
    }

    for _ in 0..iterations {
        let mut accum = vec![(0.0f32, 0.0f32, 0.0f32, 0.0f32); k];
        weights.clear();
        weights.resize(k, 0.0f32);

        for (lab, w) in samples {
            let (idx, _) = centers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, lab_distance2(*lab, *c)))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();

            accum[idx].0 += lab.l * w;
            accum[idx].1 += lab.a * w;
            accum[idx].2 += lab.b * w;
            accum[idx].3 += w;
            weights[idx] += *w;
        }

        for i in 0..k {
            if accum[i].3 > 0.0 {
                centers[i] = Lab::new(
                    accum[i].0 / accum[i].3,
                    accum[i].1 / accum[i].3,
                    accum[i].2 / accum[i].3,
                );
            }
        }
    }

    let total_weight: f32 = weights.iter().copied().sum::<f32>().max(f32::EPSILON);
    centers
        .into_iter()
        .zip(weights)
        .map(|(c, w)| (c, w / total_weight))
        .collect()
}

fn palette_similarity(ref_palette: &[(Lab, f32)], impl_palette: &[(Lab, f32)]) -> f32 {
    if ref_palette.is_empty() || impl_palette.is_empty() {
        return 0.0;
    }

    ref_palette
        .iter()
        .map(|(lab_ref, weight)| {
            let delta = impl_palette
                .iter()
                .map(|(lab_impl, _)| lab_distance2(*lab_ref, *lab_impl).sqrt())
                .fold(f32::INFINITY, f32::min);

            let match_score = 1.0 - (delta / 25.0).min(1.0);
            weight * match_score
        })
        .sum::<f32>()
}

fn palette_diffs(
    ref_palette: &[(Lab, f32)],
    impl_palette: &[(Lab, f32)],
    top_n: usize,
) -> Vec<ColorDiff> {
    if ref_palette.is_empty() || impl_palette.is_empty() {
        return Vec::new();
    }

    let mut sorted = ref_palette.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(top_n);

    let mut diffs = Vec::new();
    for (idx, (lab_ref, _w)) in sorted.iter().enumerate() {
        if let Some((lab_impl, delta)) = impl_palette
            .iter()
            .map(|(l, _)| {
                let d = lab_distance2(*lab_ref, *l).sqrt();
                (l, d)
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        {
            let kind = match idx {
                0 => ColorDiffKind::PrimaryColorShift,
                1 => ColorDiffKind::AccentColorShift,
                _ => ColorDiffKind::BackgroundColorShift,
            };
            diffs.push(ColorDiff {
                kind,
                ref_color: lab_to_hex(*lab_ref),
                impl_color: lab_to_hex(*lab_impl),
                delta_e: Some(delta),
            });
        }
    }

    diffs
}

fn average_rgb(img: &DynamicImage) -> [u8; 3] {
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for (_x, _y, pixel) in img.pixels() {
        let c = pixel.0;
        sum[0] += c[0] as u64;
        sum[1] += c[1] as u64;
        sum[2] += c[2] as u64;
        count += 1;
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ]
}

fn rgb_distance(a: &[u8; 3], b: &[u8; 3]) -> f32 {
    let dr = a[0] as f32 - b[0] as f32;
    let dg = a[1] as f32 - b[1] as f32;
    let db = a[2] as f32 - b[2] as f32;
    ((dr * dr + dg * dg + db * db).sqrt()).max(0.0)
}

fn lab_to_hex(lab: Lab) -> String {
    let srgb: Srgb = Srgb::from_color_unclamped(lab);
    let clamp = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        clamp(srgb.red),
        clamp(srgb.green),
        clamp(srgb.blue)
    )
}

fn lab_distance2(a: Lab, b: Lab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    dl * dl + da * da + db * db
}

fn extract_texts(view: &NormalizedView) -> Vec<String> {
    let mut texts = Vec::new();

    if let Some(dom) = &view.dom {
        for node in &dom.nodes {
            if let Some(text) = &node.text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
    }

    if let Some(figma) = &view.figma_tree {
        for node in &figma.nodes {
            if let Some(text) = &node.text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
    }

    if let Some(blocks) = &view.ocr_blocks {
        for block in blocks {
            let trimmed = block.text.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
    }

    texts
}

fn token_similarity(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count() as f32;
    let denom = (set_a.len() + set_b.len()) as f32;

    if denom == 0.0 {
        0.0
    } else {
        (2.0 * intersection) / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ColorMetric, ComputedStyle, ContentMetric, LayoutMetric, PixelMetric, ResourceKind,
        TypographyMetric, TypographyStyle,
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
        let impl_view = view_with_dom(vec![]); // no matching elements
        let metric = LayoutSimilarity::default();
        let err = metric
            .compute(&ref_view, &impl_view)
            .expect_err("should error on empty implementation layout");
        let msg = format!("{err:?}").to_ascii_lowercase();
        assert!(
            msg.contains("implementation"),
            "expected implementation layout error, got {msg}"
        );
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
        let ref_view = dummy_view(); // no DOM or figma
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
        let impl_view = view_with_dom(vec![]); // implementation missing all text
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
