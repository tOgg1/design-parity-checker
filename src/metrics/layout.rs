use crate::error::DpcError;
use crate::types::{BoundingBox, LayoutDiffKind, LayoutDiffRegion, LayoutMetric, NormalizedView};
use crate::Result;

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
    bbox: BoundingBox,
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

impl LayoutSimilarity {
    fn extract_elements(view: &NormalizedView) -> Vec<LayoutElement> {
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
                return elements;
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
                return elements;
            }
        }

        Vec::new()
    }

    pub fn compute_metric(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<LayoutMetric> {
        let ref_elements = LayoutSimilarity::extract_elements(reference);
        if ref_elements.is_empty() {
            return Err(DpcError::Config(
                "No layout elements available in reference view".to_string(),
            ));
        }
        let mut impl_elements = LayoutSimilarity::extract_elements(implementation);

        if impl_elements.is_empty() {
            let diff_regions = ref_elements
                .iter()
                .map(|ref_el| LayoutDiffRegion {
                    x: ref_el.bbox.x,
                    y: ref_el.bbox.y,
                    width: ref_el.bbox.width,
                    height: ref_el.bbox.height,
                    kind: LayoutDiffKind::MissingElement,
                    element_type: Some(ref_el.kind.as_str().to_string()),
                    label: None,
                })
                .collect::<Vec<_>>();

            return Ok(LayoutMetric {
                score: 0.0,
                diff_regions,
            });
        }

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

        Ok(LayoutMetric {
            score,
            diff_regions,
        })
    }
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

fn iou(a: &BoundingBox, b: &BoundingBox) -> f32 {
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
