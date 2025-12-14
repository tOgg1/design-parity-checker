use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::types::{BoundingBox, DomNode, FigmaNode, FigmaPaintKind, NormalizedView, ResourceKind};
use dpc_lib::QualityFindingType;
use dpc_lib::{
    parse_resource, DpcError, DpcOutput, FindingSeverity, QualityFinding, QualityOutput,
    ResourceDescriptor, Viewport,
};
use image::{DynamicImage, GenericImageView};

use crate::cli::OutputFormat;
use crate::formatting::{render_error, write_output};
use crate::pipeline::{resolve_artifacts_dir, resource_to_normalized_view};
use crate::settings::{flag_present, load_config};

/// Run the quality command.
#[allow(clippy::too_many_arguments)]
pub async fn run_quality(
    raw_args: &[String],
    config_path: Option<PathBuf>,
    verbose: bool,
    input: String,
    input_type: Option<crate::cli::ResourceType>,
    viewport: Viewport,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> ExitCode {
    let config = match load_config(config_path.as_deref()) {
        Ok(cfg) => cfg,
        Err(err) => return render_error(err, format, output.clone()),
    };
    let viewport = if flag_present(raw_args, "--viewport") {
        viewport
    } else {
        config.viewport
    };
    let timeouts = config.timeouts;
    let nav_timeout = timeouts.navigation.as_secs();
    let network_idle_timeout = timeouts.network_idle.as_secs();
    let process_timeout = timeouts.process.as_secs();

    if verbose {
        eprintln!("Parsing input resource…");
    }
    let input_res = match parse_resource(&input, input_type.map(resource_kind_from_cli)) {
        Ok(res) => res,
        Err(err) => return render_error(DpcError::Config(err.to_string()), format, output.clone()),
    };

    let (artifacts_dir, _from_cli) = resolve_artifacts_dir(None);
    if let Err(err) = std::fs::create_dir_all(&artifacts_dir) {
        return render_error(DpcError::Io(err), format, output.clone());
    }
    if verbose {
        eprintln!(
            "Normalizing input ({:?})… (artifacts: {})",
            input_res.kind,
            artifacts_dir.display()
        );
    }
    let progress_logger: Option<Arc<dyn Fn(&str) + Send + Sync>> = if verbose {
        Some(Arc::new(|msg: &str| eprintln!("{msg}")))
    } else {
        None
    };
    let view = match resource_to_normalized_view(
        &input_res,
        &viewport,
        &artifacts_dir,
        "input",
        progress_logger,
        nav_timeout,
        network_idle_timeout,
        process_timeout,
    )
    .await
    {
        Ok(view) => view,
        Err(err) => {
            return render_error(
                DpcError::Config(format!("Failed to process input: {err}")),
                format,
                output.clone(),
            )
        }
    };

    if verbose {
        eprintln!("Scoring quality heuristics…");
    }
    let (score, findings) = score_quality(&view, &viewport);

    let body = DpcOutput::Quality(QualityOutput {
        version: DPC_OUTPUT_VERSION.to_string(),
        input: ResourceDescriptor {
            kind: input_res.kind,
            value: input_res.value,
        },
        viewport,
        score,
        findings,
    });
    if let Err(err) = write_output(&body, format, output.clone()) {
        return render_error(DpcError::Config(err.to_string()), format, output);
    }
    ExitCode::SUCCESS
}

fn resource_kind_from_cli(rt: crate::cli::ResourceType) -> ResourceKind {
    match rt {
        crate::cli::ResourceType::Url => ResourceKind::Url,
        crate::cli::ResourceType::Image => ResourceKind::Image,
        crate::cli::ResourceType::Figma => ResourceKind::Figma,
    }
}

fn score_quality(view: &NormalizedView, viewport: &Viewport) -> (f32, Vec<QualityFinding>) {
    let mut findings = Vec::new();
    let mut score = 0.4;
    let spacing_gaps = collect_vertical_gaps(view);

    if let Some(dom) = &view.dom {
        let total_nodes = dom.nodes.len().max(1) as f32;
        score += 0.15;
        let text_nodes = dom.nodes.iter().filter(|n| node_has_text(n)).count();
        if text_nodes == 0 {
            findings.push(QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message: "No textual content detected; page may lack hierarchy.".to_string(),
            });
            score -= 0.1;
        } else {
            score += ((text_nodes as f32 / total_nodes) * 0.25).min(0.25);
        }

        let heading_nodes = dom.nodes.iter().filter(|n| is_heading(n)).count();
        if heading_nodes == 0 {
            findings.push(QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message: "No headings detected (h1-h3); add hierarchy for scannability."
                    .to_string(),
            });
            score -= 0.05;
        } else {
            score += 0.05;
        }
    } else if let Some(figma) = &view.figma_tree {
        let total_nodes = figma.nodes.len().max(1) as f32;
        score += 0.15;
        let text_nodes = figma.nodes.iter().filter(|n| figma_has_text(n)).count();
        if text_nodes == 0 {
            findings.push(QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message: "Figma snapshot has no text nodes; add copy for hierarchy.".to_string(),
            });
            score -= 0.05;
        } else {
            score += ((text_nodes as f32 / total_nodes) * 0.2).min(0.2);
        }
    } else {
        findings.push(QualityFinding {
            severity: FindingSeverity::Warning,
            finding_type: QualityFindingType::MissingHierarchy,
            message:
                "No DOM or Figma metadata available; quality scoring is limited to the screenshot."
                    .to_string(),
        });
        score -= 0.1;
    }

    if let Some(blocks) = &view.ocr_blocks {
        if !blocks.is_empty() {
            score += 0.03;
        }
    }

    let (hierarchy_delta, hierarchy_finding) = hierarchy_heuristic(view);
    score += hierarchy_delta;
    findings.push(hierarchy_finding);

    let (alignment_score, alignment_finding) = alignment_heuristic(view, viewport);
    if let Some(alignment_score) = alignment_score {
        score += alignment_score * 0.15;
    }
    findings.push(alignment_finding);

    let (contrast_score, contrast_finding) = contrast_heuristic(view);
    if let Some(contrast_score) = contrast_score {
        score += contrast_score * 0.15;
    }
    findings.push(contrast_finding);

    if let Some((finding, penalty)) = evaluate_spacing(&spacing_gaps) {
        findings.push(finding);
        score -= penalty;
    } else if spacing_gaps.len() >= 2 {
        // Mild boost when spacing looks coherent (few distinct gaps).
        score += 0.02;
    }

    (score.clamp(0.0, 1.0), findings)
}

fn alignment_heuristic(
    view: &NormalizedView,
    viewport: &Viewport,
) -> (Option<f32>, QualityFinding) {
    let min_span = (viewport.width as f32 * 0.01).clamp(4.0, 20.0);
    let mut positions: Vec<f32> = if let Some(dom) = &view.dom {
        dom.nodes
            .iter()
            .filter(|n| n.bounding_box.width >= min_span)
            .map(|n| n.bounding_box.x)
            .collect()
    } else if let Some(figma) = &view.figma_tree {
        figma
            .nodes
            .iter()
            .filter(|n| n.bounding_box.width >= min_span)
            .map(|n| n.bounding_box.x)
            .collect()
    } else {
        Vec::new()
    };

    if positions.len() < 3 {
        return (
            None,
            QualityFinding {
                severity: FindingSeverity::Info,
                finding_type: QualityFindingType::AlignmentInconsistent,
                message: "Not enough elements to assess alignment (need 3+ with bounding boxes)."
                    .to_string(),
            },
        );
    }

    positions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let tolerance = (viewport.width as f32 * 0.01).clamp(4.0, 24.0);

    let mut clusters: Vec<(f32, usize)> = Vec::new();
    for x in positions.iter().copied() {
        if let Some((center, count)) = clusters.last_mut() {
            if (x - *center).abs() <= tolerance {
                let new_count = *count + 1;
                *center = (*center * (*count as f32) + x) / new_count as f32;
                *count = new_count;
            } else {
                clusters.push((x, 1));
            }
        } else {
            clusters.push((x, 1));
        }
    }

    let centers: Vec<f32> = clusters.iter().map(|(c, _)| *c).collect();
    let mut aligned = 0usize;
    let mut outliers = 0usize;
    for x in positions.iter().copied() {
        let nearest = centers.iter().fold(f32::MAX, |acc, c| {
            let dist = (x - c).abs();
            if dist < acc {
                dist
            } else {
                acc
            }
        });
        if nearest <= tolerance * 1.5 {
            aligned += 1;
        } else {
            outliers += 1;
        }
    }

    let total = positions.len() as f32;
    let alignment_score = if total > 0.0 {
        aligned as f32 / total
    } else {
        1.0
    };

    let severity = if alignment_score < 0.75 && outliers >= 2 {
        FindingSeverity::Warning
    } else {
        FindingSeverity::Info
    };
    let message = format!(
        "{} of {} elements deviate from {} column(s) (tolerance ~{:.0}px).",
        outliers,
        positions.len(),
        centers.len(),
        tolerance
    );

    (
        Some(alignment_score),
        QualityFinding {
            severity,
            finding_type: QualityFindingType::AlignmentInconsistent,
            message,
        },
    )
}

fn hierarchy_heuristic(view: &NormalizedView) -> (f32, QualityFinding) {
    const TOLERANCE: f32 = 0.10; // 10% difference counts as a new tier
    let mut sizes = collect_font_sizes(view);

    if sizes.is_empty() {
        return (
            -0.05,
            QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message:
                    "No font size data found; add text with explicit sizes to establish hierarchy."
                        .to_string(),
            },
        );
    }

    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mut tiers: Vec<f32> = Vec::new();
    for size in sizes {
        if let Some(rep) = tiers.last().copied() {
            let delta = (size - rep).abs();
            if delta > rep * TOLERANCE {
                tiers.push(size);
            }
        } else {
            tiers.push(size);
        }
    }

    let tier_count = tiers.len();
    match tier_count {
        0 | 1 => (
            -0.08,
            QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message:
                    "Only one text size detected; add 2–3 tiers (title/subtitle/body) for hierarchy."
                        .to_string(),
            },
        ),
        2 | 3 => (
            0.08,
            QualityFinding {
                severity: FindingSeverity::Info,
                finding_type: QualityFindingType::MissingHierarchy,
                message: format!(
                    "Hierarchy looks healthy with {} distinct text size tier(s).",
                    tier_count
                ),
            },
        ),
        _ => (
            -0.04,
            QualityFinding {
                severity: FindingSeverity::Warning,
                finding_type: QualityFindingType::MissingHierarchy,
                message: format!(
                    "Found {} distinct text sizes; consolidate to 2–3 tiers for clearer hierarchy.",
                    tier_count
                ),
            },
        ),
    }
}

fn collect_font_sizes(view: &NormalizedView) -> Vec<f32> {
    let mut sizes = Vec::new();

    if let Some(dom) = &view.dom {
        for node in dom.nodes.iter().filter(|n| node_has_text(n)) {
            if let Some(size) = node
                .computed_style
                .as_ref()
                .and_then(|style| style.font_size)
            {
                sizes.push(size);
            }
        }
    }

    if let Some(figma) = &view.figma_tree {
        for node in figma.nodes.iter().filter(|n| figma_has_text(n)) {
            if let Some(size) = node
                .typography
                .as_ref()
                .and_then(|style| style.font_size)
            {
                sizes.push(size);
            }
        }
    }

    sizes
}

fn contrast_heuristic(view: &NormalizedView) -> (Option<f32>, QualityFinding) {
    let img = match image::open(&view.screenshot_path) {
        Ok(img) => img,
        Err(err) => {
            return (
                None,
                QualityFinding {
                    severity: FindingSeverity::Info,
                    finding_type: QualityFindingType::LowContrast,
                    message: format!(
                        "Could not read screenshot for contrast heuristic: {}",
                        err
                    ),
                },
            )
        }
    };

    let mut ratios = Vec::new();

    if let Some(dom) = &view.dom {
        for node in dom.nodes.iter().filter(|n| node_has_text(n)) {
            if let Some(r) = contrast_for_dom_node(node, &img, view) {
                ratios.push(r);
            }
        }
    }

    if let Some(figma) = &view.figma_tree {
        for node in figma.nodes.iter().filter(|n| figma_has_text(n)) {
            if let Some(r) = contrast_for_figma_node(node, &img, view) {
                ratios.push(r);
            }
        }
    }

    if ratios.is_empty() {
        return (
            None,
            QualityFinding {
                severity: FindingSeverity::Info,
                finding_type: QualityFindingType::LowContrast,
                message: "Not enough text samples to assess contrast (missing color data)."
                    .to_string(),
            },
        );
    }

    let threshold = 4.0;
    let low = ratios.iter().filter(|r| **r < threshold).count();
    let worst = ratios
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min)
        .max(0.0);

    let contrast_score = (ratios.len().saturating_sub(low) as f32 / ratios.len() as f32).clamp(0.0, 1.0);
    let severity = if low >= 3 || worst < 3.0 {
        FindingSeverity::Warning
    } else {
        FindingSeverity::Info
    };
    let message = format!(
        "{} of {} text samples below {:.1} contrast (worst {:.1}). Aim for ≥4.5.",
        low,
        ratios.len(),
        threshold,
        worst
    );

    (
        Some(contrast_score),
        QualityFinding {
            severity,
            finding_type: QualityFindingType::LowContrast,
            message,
        },
    )
}

fn contrast_for_dom_node(
    node: &DomNode,
    img: &DynamicImage,
    view: &NormalizedView,
) -> Option<f32> {
    let style = node.computed_style.as_ref()?;
    let text = style
        .color
        .as_deref()
        .and_then(parse_css_color)
        .filter(|c| c[3] >= 0.05)?;

    let mut background = style
        .background_color
        .as_deref()
        .and_then(parse_css_color)
        .filter(|c| c[3] >= 0.05)
        .map(|c| [c[0], c[1], c[2]]);

    if background.is_none() {
        background = sample_background_color(img, &node.bounding_box, view);
    }

    let background = background?;
    let text_color = blend_over_background([text[0], text[1], text[2]], background, text[3]);
    Some(contrast_ratio(text_color, background))
}

fn contrast_for_figma_node(
    node: &FigmaNode,
    img: &DynamicImage,
    view: &NormalizedView,
) -> Option<f32> {
    let fill = node
        .fills
        .iter()
        .find(|p| p.kind == FigmaPaintKind::Solid)
        .and_then(|p| {
            p.color
                .as_deref()
                .and_then(parse_css_color)
                .map(|mut c| {
                    if let Some(opacity) = p.opacity {
                        c[3] *= opacity;
                    }
                    c
                })
        })
        .filter(|c| c[3] >= 0.05)?;

    let background = sample_background_color(img, &node.bounding_box, view)?;
    let text_color = blend_over_background([fill[0], fill[1], fill[2]], background, fill[3]);
    Some(contrast_ratio(text_color, background))
}

fn parse_css_color(value: &str) -> Option<[f32; 4]> {
    let v = value.trim().to_ascii_lowercase();
    if v == "transparent" {
        return None;
    }

    if let Some(hex) = v.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex
                .chars()
                .map(|c| format!("{c}{c}"))
                .collect::<Vec<_>>()
                .join(""),
            6 | 8 => hex.to_string(),
            _ => return None,
        };
        let r = u8::from_str_radix(&expanded[0..2], 16).ok()?;
        let g = u8::from_str_radix(&expanded[2..4], 16).ok()?;
        let b = u8::from_str_radix(&expanded[4..6], 16).ok()?;
        let a = if expanded.len() == 8 {
            u8::from_str_radix(&expanded[6..8], 16).ok()? as f32 / 255.0
        } else {
            1.0
        };
        return Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]);
    }

    if let Some(body) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let cleaned = body.trim_end_matches(')').replace('/', " ");
        let parts: Vec<_> = cleaned
            .split(|c| c == ',' || c == ' ')
            .filter(|p| !p.trim().is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let r: f32 = parts.get(0)?.trim().parse::<f32>().ok()? / 255.0;
        let g: f32 = parts.get(1)?.trim().parse::<f32>().ok()? / 255.0;
        let b: f32 = parts.get(2)?.trim().parse::<f32>().ok()? / 255.0;
        let a: f32 = if let Some(alpha) = parts.get(3) {
            alpha
                .trim()
                .parse::<f32>()
                .ok()
                .map(|v| v.clamp(0.0, 1.0))
                .unwrap_or(1.0)
        } else {
            1.0
        };
        return Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a]);
    }

    None
}

fn contrast_ratio(fg: [f32; 3], bg: [f32; 3]) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    if l1 >= l2 {
        (l1 + 0.05) / (l2 + 0.05)
    } else {
        (l2 + 0.05) / (l1 + 0.05)
    }
}

fn relative_luminance(rgb: [f32; 3]) -> f32 {
    fn to_linear(c: f32) -> f32 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = to_linear(rgb[0].clamp(0.0, 1.0));
    let g = to_linear(rgb[1].clamp(0.0, 1.0));
    let b = to_linear(rgb[2].clamp(0.0, 1.0));
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn blend_over_background(fg: [f32; 3], bg: [f32; 3], alpha: f32) -> [f32; 3] {
    let a = alpha.clamp(0.0, 1.0);
    [
        fg[0] * a + bg[0] * (1.0 - a),
        fg[1] * a + bg[1] * (1.0 - a),
        fg[2] * a + bg[2] * (1.0 - a),
    ]
}

fn sample_background_color(
    img: &DynamicImage,
    bbox: &BoundingBox,
    view: &NormalizedView,
) -> Option<[f32; 3]> {
    let (x0, y0, w, h) = bbox_to_pixels(bbox, view.width, view.height)?;
    if w == 0 || h == 0 {
        return None;
    }
    let step_x = (w / 12).max(1);
    let step_y = (h / 12).max(1);

    let mut accum = [0u64; 3];
    let mut count = 0u64;
    for y in (y0..y0 + h).step_by(step_y as usize) {
        for x in (x0..x0 + w).step_by(step_x as usize) {
            let pixel = img.get_pixel(x, y).0;
            accum[0] += pixel[0] as u64;
            accum[1] += pixel[1] as u64;
            accum[2] += pixel[2] as u64;
            count += 1;
        }
    }

    if count == 0 {
        None
    } else {
        Some([
            accum[0] as f32 / count as f32 / 255.0,
            accum[1] as f32 / count as f32 / 255.0,
            accum[2] as f32 / count as f32 / 255.0,
        ])
    }
}

fn bbox_to_pixels(
    bbox: &BoundingBox,
    view_width: u32,
    view_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if view_width == 0 || view_height == 0 {
        return None;
    }

    let normalized = bbox.width <= 1.5
        && bbox.height <= 1.5
        && bbox.x >= 0.0
        && bbox.x <= 1.5
        && bbox.y >= 0.0
        && bbox.y <= 1.5;

    let x = if normalized {
        bbox.x * view_width as f32
    } else {
        bbox.x
    };
    let y = if normalized {
        bbox.y * view_height as f32
    } else {
        bbox.y
    };
    let mut w = if normalized {
        bbox.width * view_width as f32
    } else {
        bbox.width
    };
    let mut h = if normalized {
        bbox.height * view_height as f32
    } else {
        bbox.height
    };

    if w < 1.0 || h < 1.0 {
        w = w.max(1.0);
        h = h.max(1.0);
    }

    let x0 = x.max(0.0).floor() as u32;
    let y0 = y.max(0.0).floor() as u32;
    let x1 = (x + w).ceil().min(view_width as f32) as u32;
    let y1 = (y + h).ceil().min(view_height as f32) as u32;

    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some((x0.min(view_width - 1), y0.min(view_height - 1), x1 - x0, y1 - y0))
    }
}

fn node_has_text(node: &DomNode) -> bool {
    node.text
        .as_ref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false)
}

fn is_heading(node: &DomNode) -> bool {
    matches!(node.tag.to_ascii_lowercase().as_str(), "h1" | "h2" | "h3")
}

fn figma_has_text(node: &FigmaNode) -> bool {
    node.text
        .as_ref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false)
}

fn collect_vertical_gaps(view: &NormalizedView) -> Vec<f32> {
    let mut boxes: Vec<_> = if let Some(dom) = &view.dom {
        dom.nodes.iter().map(|n| n.bounding_box).collect()
    } else if let Some(figma) = &view.figma_tree {
        figma.nodes.iter().map(|n| n.bounding_box).collect()
    } else {
        Vec::new()
    };

    boxes.retain(|b| b.height > 0.0);
    if boxes.len() < 2 {
        return Vec::new();
    }

    boxes.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(Ordering::Equal))
    });

    let mut gaps = Vec::new();
    for window in boxes.windows(2) {
        if let [first, second] = window {
            let bottom = first.y + first.height;
            let gap = second.y - bottom;
            if gap > 0.001 {
                gaps.push(gap);
            }
        }
    }
    gaps
}

fn evaluate_spacing(gaps: &[f32]) -> Option<(QualityFinding, f32)> {
    if gaps.len() < 5 {
        return None;
    }

    let mut buckets: HashMap<i32, usize> = HashMap::new();
    for gap in gaps {
        let bucket = (gap * 100.0).round() as i32; // bucket by ~1% height
        *buckets.entry(bucket).or_insert(0) += 1;
    }

    let distinct = buckets.len();
    if distinct < 5 {
        return None;
    }

    let total = gaps.len() as f32;
    let max_bucket = buckets.values().copied().max().unwrap_or(0) as f32;
    let outlier_ratio = if total > 0.0 {
        1.0 - (max_bucket / total)
    } else {
        0.0
    };

    let min_gap = gaps
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min)
        .min(1.0);
    let max_gap = gaps
        .iter()
        .copied()
        .fold(0.0f32, f32::max)
        .min(1.0);

    let penalty = (0.05 + outlier_ratio * 0.1).min(0.15);
    let finding = QualityFinding {
        severity: FindingSeverity::Warning,
        finding_type: QualityFindingType::SpacingInconsistent,
        message: format!(
            "Spacing appears inconsistent: {} distinct vertical gaps across {} samples (min {:.1}%, max {:.1}%).",
            distinct,
            gaps.len(),
            min_gap * 100.0,
            max_gap * 100.0
        ),
    };

    Some((finding, penalty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpc_lib::types::{BoundingBox, ComputedStyle, DomSnapshot, ResourceKind};
    use image::{ImageBuffer, Rgba};
    use std::collections::HashMap;

    fn view_with_boxes(boxes: Vec<BoundingBox>) -> NormalizedView {
        let nodes = boxes
            .into_iter()
            .enumerate()
            .map(|(idx, bbox)| DomNode {
                id: format!("n{idx}"),
                tag: "div".to_string(),
                children: Vec::new(),
                parent: None,
                attributes: HashMap::new(),
                text: None,
                bounding_box: bbox,
                computed_style: None,
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
                nodes,
            }),
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    #[test]
    fn flags_spacing_when_many_distinct_gaps() {
        let view = view_with_boxes(vec![
            BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.1,
                y: 0.15,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.2,
                y: 0.27,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.42,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.6,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.8,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.93,
                width: 0.2,
                height: 0.05,
            },
        ]);

        let (_score, findings) = score_quality(&view, &Viewport { width: 800, height: 600 });
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.finding_type, QualityFindingType::SpacingInconsistent)),
            "expected spacing finding when many distinct gaps are present"
        );
    }

    #[test]
    fn does_not_flag_spacing_with_consistent_gaps() {
        let view = view_with_boxes(vec![
            BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.15,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.1,
                y: 0.3,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.1,
                y: 0.45,
                width: 0.2,
                height: 0.1,
            },
        ]);

        let (_score, findings) = score_quality(&view, &Viewport { width: 800, height: 600 });
        assert!(
            !findings
                .iter()
                .any(|f| matches!(f.finding_type, QualityFindingType::SpacingInconsistent)),
            "should not flag spacing when gaps are consistent and few distinct values"
        );
    }

    fn view_with_font_sizes(sizes: &[f32]) -> NormalizedView {
        let nodes = sizes
            .iter()
            .enumerate()
            .map(|(idx, size)| DomNode {
                id: format!("t{idx}"),
                tag: "p".to_string(),
                children: Vec::new(),
                parent: None,
                attributes: HashMap::new(),
                text: Some(format!("Text {idx}")),
                bounding_box: BoundingBox {
                    x: 0.0,
                    y: idx as f32 * 20.0,
                    width: 50.0,
                    height: 10.0,
                },
                computed_style: Some(ComputedStyle {
                    font_size: Some(*size),
                    ..ComputedStyle::default()
                }),
            })
            .collect();

        NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: "dummy.png".into(),
            width: 800,
            height: 600,
            dom: Some(DomSnapshot {
                url: None,
                title: None,
                nodes,
            }),
            figma_tree: None,
            ocr_blocks: None,
        }
    }

    #[test]
    fn hierarchy_scores_tiered_text_higher() {
        let tiered = view_with_font_sizes(&[32.0, 20.0, 16.0]);
        let flat = view_with_font_sizes(&[16.0, 16.0, 16.0]);

        let (tiered_score, tiered_findings) =
            score_quality(&tiered, &Viewport { width: 800, height: 600 });
        let (flat_score, flat_findings) =
            score_quality(&flat, &Viewport { width: 800, height: 600 });

        assert!(
            tiered_score > flat_score,
            "tiered text should yield a higher hierarchy score"
        );
        assert!(
            flat_findings.iter().any(|f| {
                matches!(
                    f.finding_type,
                    QualityFindingType::MissingHierarchy
                ) && matches!(f.severity, FindingSeverity::Warning)
            }),
            "flat hierarchy should trigger a missing_hierarchy warning"
        );
        assert!(
            tiered_findings
                .iter()
                .any(|f| matches!(f.finding_type, QualityFindingType::MissingHierarchy)),
            "hierarchy finding should be present even when healthy"
        );
    }

    #[test]
    fn flags_low_contrast_text_against_background() {
        let tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp image");
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_pixel(120, 80, Rgba([235, 235, 235, 255]));
        img.save(tmp.path()).unwrap();

        let node = DomNode {
            id: "text1".into(),
            tag: "p".into(),
            children: Vec::new(),
            parent: None,
            attributes: HashMap::new(),
            text: Some("hello".into()),
            bounding_box: BoundingBox {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 20.0,
            },
            computed_style: Some(ComputedStyle {
                font_family: None,
                font_size: None,
                font_weight: None,
                line_height: None,
                color: Some("rgb(210, 210, 210)".to_string()),
                background_color: None,
                display: None,
                visibility: None,
                opacity: Some(1.0),
            }),
        };

        let view = NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: tmp.path().to_path_buf(),
            width: 120,
            height: 80,
            dom: Some(DomSnapshot {
                url: None,
                title: None,
                nodes: vec![node],
            }),
            figma_tree: None,
            ocr_blocks: None,
        };

        let (_score, findings) = score_quality(&view, &Viewport { width: 120, height: 80 });
        let finding = findings
            .iter()
            .find(|f| matches!(f.finding_type, QualityFindingType::LowContrast))
            .expect("low contrast finding");
        assert!(
            matches!(finding.severity, FindingSeverity::Warning),
            "expected warning severity for low contrast, got {:?}",
            finding.severity
        );
        assert!(
            finding.message.to_ascii_lowercase().contains("contrast"),
            "expected contrast context in message: {}",
            finding.message
        );
    }

    #[test]
    fn treats_high_contrast_as_informational() {
        let tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp image");
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_pixel(100, 60, Rgba([255, 255, 255, 255]));
        img.save(tmp.path()).unwrap();

        let node = DomNode {
            id: "text2".into(),
            tag: "p".into(),
            children: Vec::new(),
            parent: None,
            attributes: HashMap::new(),
            text: Some("hello".into()),
            bounding_box: BoundingBox {
                x: 5.0,
                y: 5.0,
                width: 50.0,
                height: 18.0,
            },
            computed_style: Some(ComputedStyle {
                font_family: None,
                font_size: None,
                font_weight: None,
                line_height: None,
                color: Some("rgb(30, 30, 30)".to_string()),
                background_color: None,
                display: None,
                visibility: None,
                opacity: Some(1.0),
            }),
        };

        let view = NormalizedView {
            kind: ResourceKind::Image,
            screenshot_path: tmp.path().to_path_buf(),
            width: 100,
            height: 60,
            dom: Some(DomSnapshot {
                url: None,
                title: None,
                nodes: vec![node],
            }),
            figma_tree: None,
            ocr_blocks: None,
        };

        let (_score, findings) = score_quality(&view, &Viewport { width: 100, height: 60 });
        let finding = findings
            .iter()
            .find(|f| matches!(f.finding_type, QualityFindingType::LowContrast))
            .expect("low contrast finding");
        assert!(
            matches!(finding.severity, FindingSeverity::Info),
            "expected informational severity for acceptable contrast, got {:?}",
            finding.severity
        );
    }
}
