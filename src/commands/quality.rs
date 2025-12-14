use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::types::{DomNode, FigmaNode, NormalizedView, ResourceKind};
use dpc_lib::QualityFindingType;
use dpc_lib::{
    parse_resource, DpcError, DpcOutput, FindingSeverity, QualityFinding, QualityOutput,
    ResourceDescriptor, Viewport,
};

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

    let (alignment_score, alignment_finding) = alignment_heuristic(view, viewport);
    if let Some(alignment_score) = alignment_score {
        score += alignment_score * 0.15;
    }
    findings.push(alignment_finding);

    if let Some((finding, penalty)) = evaluate_spacing(&spacing_gaps) {
        findings.push(finding);
        score -= penalty;
    } else if spacing_gaps.len() >= 2 {
        // Mild boost when spacing looks coherent (few distinct gaps).
        score += 0.02;
    }
    findings.push(QualityFinding {
        severity: FindingSeverity::Info,
        finding_type: QualityFindingType::LowContrast,
        message: "Contrast heuristic not implemented yet (see design-parity-checker-vqg)."
            .to_string(),
    });

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
    use dpc_lib::types::{DomSnapshot, ResourceKind};

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
                y: 0.32,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.5,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.68,
                width: 0.2,
                height: 0.1,
            },
            BoundingBox {
                x: 0.05,
                y: 0.87,
                width: 0.2,
                height: 0.1,
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
}
