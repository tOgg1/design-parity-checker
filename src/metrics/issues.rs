use crate::types::{
    ColorDiffKind, ColorMetric, ContentMetric, DiffSeverity, LayoutDiffKind, LayoutMetric,
    MetricScores, PixelMetric, TypographyIssue, TypographyMetric,
};

const PRIORITY_PIXEL: u8 = 0;
const PRIORITY_LAYOUT: u8 = 1;
const PRIORITY_CONTENT: u8 = 2;
const PRIORITY_COLOR: u8 = 3;
const PRIORITY_TYPOGRAPHY: u8 = 4;

#[derive(Debug, Clone)]
struct RankedIssue {
    severity_rank: u8,
    priority_rank: u8,
    message: String,
}

impl RankedIssue {
    fn new(severity_rank: u8, priority_rank: u8, message: impl Into<String>) -> Self {
        Self {
            severity_rank,
            priority_rank,
            message: message.into(),
        }
    }

    fn major(priority_rank: u8, message: impl Into<String>) -> Self {
        Self::new(0, priority_rank, message)
    }

    fn moderate(priority_rank: u8, message: impl Into<String>) -> Self {
        Self::new(1, priority_rank, message)
    }

    fn minor(priority_rank: u8, message: impl Into<String>) -> Self {
        Self::new(2, priority_rank, message)
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
            .then_with(|| a.priority_rank.cmp(&b.priority_rank))
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
        issues.push(RankedIssue::major(
            PRIORITY_PIXEL,
            format!(
                "{} major pixel difference region{} detected.",
                major_count,
                if major_count == 1 { "" } else { "s" }
            ),
        ));
    }
    if moderate_count > 0 {
        issues.push(RankedIssue::moderate(
            PRIORITY_PIXEL,
            format!(
                "{} moderate pixel difference region{} detected.",
                moderate_count,
                if moderate_count == 1 { "" } else { "s" }
            ),
        ));
    }
    if minor_count > 0 {
        issues.push(RankedIssue::minor(
            PRIORITY_PIXEL,
            format!(
                "{} minor pixel difference region{} detected.",
                minor_count,
                if minor_count == 1 { "" } else { "s" }
            ),
        ));
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
            LayoutDiffKind::MissingElement => RankedIssue::major(PRIORITY_LAYOUT, msg),
            LayoutDiffKind::ExtraElement => RankedIssue::moderate(PRIORITY_LAYOUT, msg),
            LayoutDiffKind::PositionShift | LayoutDiffKind::SizeChange => {
                RankedIssue::moderate(PRIORITY_LAYOUT, msg)
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
            RankedIssue::major(PRIORITY_TYPOGRAPHY, msg)
        } else if diff.issues.contains(&TypographyIssue::FontSizeDiff)
            || diff.issues.contains(&TypographyIssue::FontWeightDiff)
        {
            RankedIssue::moderate(PRIORITY_TYPOGRAPHY, msg)
        } else {
            RankedIssue::minor(PRIORITY_TYPOGRAPHY, msg)
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
            ColorDiffKind::PrimaryColorShift => RankedIssue::major(PRIORITY_COLOR, msg),
            ColorDiffKind::AccentColorShift => RankedIssue::major(PRIORITY_COLOR, msg),
            ColorDiffKind::BackgroundColorShift => RankedIssue::minor(PRIORITY_COLOR, msg),
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
                issues.push(RankedIssue::major(
                    PRIORITY_CONTENT,
                    format!("Text '{}' is missing in the implementation.", truncated),
                ));
            }
        } else {
            issues.push(RankedIssue::major(
                PRIORITY_CONTENT,
                format!(
                    "{} text elements are missing in the implementation.",
                    count
                ),
            ));
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
                issues.push(RankedIssue::minor(
                    PRIORITY_CONTENT,
                    format!(
                        "Extra text '{}' appears in implementation but not in design.",
                        truncated
                    ),
                ));
            }
        } else {
            issues.push(RankedIssue::minor(
                PRIORITY_CONTENT,
                format!(
                    "{} extra text elements appear in implementation but not in design.",
                    count
                ),
            ));
        }
    }

    issues
}
