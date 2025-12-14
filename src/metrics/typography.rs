use crate::error::DpcError;
use crate::types::{NormalizedView, TypographyDiff, TypographyIssue, TypographyMetric};
use crate::Result;
use std::collections::HashMap;

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

    pub fn compute_metric(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<TypographyMetric> {
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

        Ok(TypographyMetric { score, diffs })
    }
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

fn normalize_label(input: &str) -> Option<String> {
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
