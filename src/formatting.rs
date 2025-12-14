use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::{DpcError, DpcOutput, ErrorOutput, QualityFindingType};

use crate::cli::OutputFormat;

/// Write output in the requested format.
pub fn write_output(
    body: &DpcOutput,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Json => write_json_output(body, output.as_deref())?,
        OutputFormat::Pretty => write_pretty_output(body, output.as_deref())?,
    };
    Ok(())
}

/// Render an error and return the appropriate exit code.
pub fn render_error(err: DpcError, format: OutputFormat, output: Option<PathBuf>) -> ExitCode {
    let error_payload = err.to_payload();
    let payload = DpcOutput::Error(ErrorOutput {
        version: DPC_OUTPUT_VERSION.to_string(),
        message: Some(error_payload.message.clone()),
        error: error_payload,
    });

    match format {
        OutputFormat::Json => {
            let content =
                serde_json::to_string(&payload).unwrap_or_else(|_| "{\"mode\":\"error\"}".into());
            if let Some(path) = output {
                if let Err(write_err) = std::fs::write(&path, &content) {
                    eprintln!("Failed to write error output: {}", write_err);
                    println!("{content}");
                }
            } else {
                println!("{content}");
            }
        }
        OutputFormat::Pretty => {
            if let Err(write_err) = write_pretty_output(&payload, output.as_deref()) {
                eprintln!("Failed to write error output: {}", write_err);
            }
        }
    };

    // Reserve exit code 2 for fatal/errors; threshold failures use 1.
    ExitCode::from(2)
}

/// Write JSON output to file or stdout.
fn write_json_output(
    body: &DpcOutput,
    output: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = serde_json::to_string(body)?;
    if let Some(path) = output {
        std::fs::write(path, content)?;
    } else {
        println!("{content}");
    }
    Ok(())
}

/// Write pretty output to file or stdout.
fn write_pretty_output(body: &DpcOutput, output: Option<&Path>) -> io::Result<()> {
    let stdout_is_tty = std::io::stdout().is_terminal();
    let use_human = output.is_none() && stdout_is_tty;

    if use_human {
        let content = format_pretty(body, true);
        println!("{content}");
        return Ok(());
    }

    // Non-tty or file output: keep JSON shape for pipelines/files.
    let content =
        serde_json::to_string_pretty(body).unwrap_or_else(|_| "{\"mode\":\"error\"}".to_string());
    if let Some(path) = output {
        std::fs::write(path, &content)?;
    } else {
        println!("{content}");
    }
    Ok(())
}

/// Format output for human consumption in a terminal.
pub fn format_pretty(body: &DpcOutput, colorize: bool) -> String {
    let format_score = |score: f32, threshold: Option<f32>| {
        let pct = score * 100.0;
        let text = format!("{:.3}", score);
        let code = if let Some(th) = threshold {
            if score >= th {
                "32"
            } else if (th - score) <= 0.05 {
                "33"
            } else {
                "31"
            }
        } else {
            score_color_code(score)
        };
        let pct_text = format!("{} ({:.1}%)", text, pct);
        color(&pct_text, code, colorize)
    };

    match body {
        DpcOutput::Compare(out) => {
            let mut buf = String::new();
            let status = if out.passed { "PASS" } else { "FAIL" };
            let status_colored = color(status, if out.passed { "32" } else { "31" }, colorize);
            let similarity = format_score(out.similarity, Some(out.threshold));
            let threshold = format!("{:.1}%", out.threshold * 100.0);
            let header = format!("{} Design parity check", status_colored);
            writeln!(buf, "{header}").ok();
            writeln!(buf, "Similarity: {similarity} (threshold {threshold})").ok();

            let mut issues: Vec<String> = out
                .summary
                .as_ref()
                .map(|s| s.top_issues.clone())
                .unwrap_or_default();
            if issues.len() > 5 {
                issues.truncate(5);
            }
            if !issues.is_empty() {
                writeln!(buf, "Top issues (max 5):").ok();
                for issue in issues {
                    writeln!(buf, "- {issue}").ok();
                }
            }

            let mut metrics: Vec<(&str, f32)> = Vec::new();
            if let Some(pixel) = &out.metrics.pixel {
                metrics.push(("pixel", pixel.score));
            }
            if let Some(layout) = &out.metrics.layout {
                metrics.push(("layout", layout.score));
            }
            if let Some(typography) = &out.metrics.typography {
                metrics.push(("typography", typography.score));
            }
            if let Some(color_metric) = &out.metrics.color {
                metrics.push(("color", color_metric.score));
            }
            if let Some(content) = &out.metrics.content {
                metrics.push(("content", content.score));
            }
            if !metrics.is_empty() {
                writeln!(buf, "Metrics:").ok();
                for (name, score) in metrics {
                    let styled = format_score(score, None);
                    writeln!(buf, "- {:12} {}", name, styled).ok();
                }
            }

            if let Some(art) = &out.artifacts {
                let mut paths = Vec::new();
                paths.push(("directory", art.directory.clone()));
                if let Some(p) = &art.ref_screenshot {
                    paths.push(("refScreenshot", p.clone()));
                }
                if let Some(p) = &art.impl_screenshot {
                    paths.push(("implScreenshot", p.clone()));
                }
                if let Some(p) = &art.diff_image {
                    paths.push(("diffImage", p.clone()));
                }
                if let Some(p) = &art.ref_dom_snapshot {
                    paths.push(("refDomSnapshot", p.clone()));
                }
                if let Some(p) = &art.impl_dom_snapshot {
                    paths.push(("implDomSnapshot", p.clone()));
                }
                if !paths.is_empty() {
                    writeln!(buf, "Artifacts:").ok();
                    for (label, path) in paths {
                        writeln!(buf, "- {:16} {}", label, path.display()).ok();
                    }
                    let mut hints = Vec::new();
                    if art.diff_image.is_some() {
                        hints.push("diff_heatmap.png");
                    }
                    if art.ref_dom_snapshot.is_some() || art.impl_dom_snapshot.is_some() {
                        hints.push("ref_dom.json/impl_dom.json");
                    }
                    if art.ref_figma_snapshot.is_some() || art.impl_figma_snapshot.is_some() {
                        hints.push("ref_figma.json/impl_figma.json");
                    }
                    if !hints.is_empty() {
                        writeln!(
                            buf,
                            "Hint: open {} in {}",
                            hints.join(", "),
                            art.directory.display()
                        )
                        .ok();
                    }
                }
            }

            buf
        }
        DpcOutput::GenerateCode(out) => {
            let mut buf = String::new();
            let header = color("[GENERATE]", "36", colorize);
            writeln!(buf, "{} Code generated", header).ok();
            writeln!(
                buf,
                "Input: {} (kind: {:?})",
                out.input.value, out.input.kind
            )
            .ok();
            if let Some(stack) = &out.stack {
                writeln!(buf, "Stack: {}", stack).ok();
            }
            if let Some(path) = &out.output_path {
                writeln!(buf, "Saved to: {}", path.display()).ok();
            } else {
                writeln!(buf, "Tip: pass --output to write the code to disk.").ok();
            }
            if let Some(code) = &out.code {
                let preview: String = code.lines().take(3).collect::<Vec<_>>().join("\n");
                writeln!(buf, "Code length: {} chars", code.len()).ok();
                if !preview.is_empty() {
                    writeln!(buf, "Preview:").ok();
                    writeln!(buf, "{}", preview).ok();
                }
            }
            if let Some(summary) = &out.summary {
                if !summary.top_issues.is_empty() {
                    writeln!(buf, "Notes:").ok();
                    for issue in &summary.top_issues {
                        writeln!(buf, "- {}", issue).ok();
                    }
                }
            }
            buf
        }
        DpcOutput::Quality(out) => {
            let mut buf = String::new();
            let header = color("[QUALITY]", "34", colorize);
            writeln!(buf, "{} Score {:.1}", header, out.score * 100.0).ok();
            writeln!(
                buf,
                "Input: {} (kind: {:?})",
                out.input.value, out.input.kind
            )
            .ok();
            if !out.findings.is_empty() {
                writeln!(buf, "Findings:").ok();
                for finding in &out.findings {
                    let severity = format!("{:?}", finding.severity).to_ascii_lowercase();
                    let kind = match finding.finding_type {
                        QualityFindingType::AlignmentInconsistent => "alignment_inconsistent",
                        QualityFindingType::SpacingInconsistent => "spacing_inconsistent",
                        QualityFindingType::LowContrast => "low_contrast",
                        QualityFindingType::MissingHierarchy => "missing_hierarchy",
                    };
                    writeln!(buf, "- [{}] {}: {}", severity, kind, finding.message).ok();
                }
            }
            buf
        }
        DpcOutput::Error(out) => {
            let mut buf = String::new();
            let header = color("[ERROR]", "31", colorize);
            let message = out
                .message
                .as_deref()
                .unwrap_or_else(|| out.error.message.as_str());
            writeln!(buf, "{} {}", header, message).ok();
            if let Some(remediation) = &out.error.remediation {
                writeln!(buf, "Hint: {}", remediation).ok();
            }
            buf
        }
    }
}

/// Apply ANSI color codes when enabled.
fn color(text: &str, code: &str, colorize: bool) -> String {
    if colorize {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

/// Map score to ANSI color code.
fn score_color_code(score: f32) -> &'static str {
    if score >= 0.9 {
        "32" // green
    } else if score >= 0.75 {
        "33" // yellow
    } else {
        "31" // red
    }
}

/// Determine exit code for compare command.
pub fn exit_code_for_compare(passed: bool) -> ExitCode {
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpc_lib::output::{CompareOutput, ResourceDescriptor, Summary};
    use dpc_lib::types::{
        ColorMetric, LayoutMetric, MetricScores, PixelMetric, ResourceKind, Viewport,
    };
    use dpc_lib::CompareArtifacts;
    use std::path::PathBuf;

    #[test]
    fn exit_code_for_compare_maps_pass_fail() {
        assert_eq!(exit_code_for_compare(true), ExitCode::SUCCESS);
        assert_eq!(exit_code_for_compare(false), ExitCode::from(1));
    }

    #[test]
    fn render_error_always_returns_fatal_exit_code() {
        let code = render_error(
            DpcError::Config("boom".to_string()),
            OutputFormat::Json,
            None,
        );
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn format_pretty_includes_status_metrics_and_artifacts() {
        let metrics = MetricScores {
            pixel: Some(PixelMetric {
                score: 0.99,
                diff_regions: vec![],
            }),
            layout: Some(LayoutMetric {
                score: 0.75,
                diff_regions: vec![],
            }),
            typography: None,
            color: Some(ColorMetric {
                score: 0.80,
                diffs: vec![],
            }),
            content: None,
        };
        let artifacts = CompareArtifacts {
            directory: PathBuf::from("/tmp/dpc-run"),
            kept: true,
            ref_screenshot: Some(PathBuf::from("/tmp/dpc-run/ref.png")),
            impl_screenshot: Some(PathBuf::from("/tmp/dpc-run/impl.png")),
            diff_image: Some(PathBuf::from("/tmp/dpc-run/diff.png")),
            ref_dom_snapshot: None,
            impl_dom_snapshot: None,
            ref_figma_snapshot: None,
            impl_figma_snapshot: None,
        };
        let output = DpcOutput::Compare(CompareOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            ref_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "ref.png".into(),
            },
            impl_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "impl.png".into(),
            },
            viewport: Viewport {
                width: 1440,
                height: 900,
            },
            similarity: 0.96,
            threshold: 0.95,
            passed: true,
            metrics,
            summary: Some(Summary {
                top_issues: vec!["Design parity check passed".into()],
            }),
            artifacts: Some(artifacts),
        });

        let pretty = format_pretty(&output, false);
        assert!(pretty.contains("PASS Design parity check"));
        assert!(pretty.contains("Similarity"));
        assert!(pretty.contains("Metrics:"));
        assert!(pretty.contains("pixel") && pretty.contains("0.99"));
        assert!(pretty.contains("layout") && pretty.contains("0.75"));
        assert!(pretty.contains("color") && pretty.contains("0.80"));
        assert!(pretty.contains("Artifacts:"));
        assert!(pretty.contains("refScreenshot"));
    }

    #[test]
    fn format_pretty_includes_status_and_metrics_simple() {
        let output = DpcOutput::Compare(CompareOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            ref_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "ref.png".to_string(),
            },
            impl_resource: ResourceDescriptor {
                kind: ResourceKind::Image,
                value: "impl.png".to_string(),
            },
            viewport: Viewport {
                width: 800,
                height: 600,
            },
            similarity: 0.96,
            threshold: 0.95,
            passed: true,
            metrics: MetricScores {
                pixel: Some(PixelMetric {
                    score: 0.96,
                    diff_regions: Vec::new(),
                }),
                layout: None,
                typography: None,
                color: None,
                content: None,
            },
            summary: Some(Summary {
                top_issues: vec![
                    "Design parity check passed (96.0% similarity, threshold: 95.0%)".into(),
                ],
            }),
            artifacts: None,
        });

        let pretty = format_pretty(&output, false);
        assert!(pretty.contains("PASS Design parity check"));
        assert!(pretty.contains("Similarity"));
        assert!(pretty.contains("threshold"));
        assert!(pretty.contains("Metrics:"));
        assert!(pretty.contains("pixel") && pretty.contains("0.96"));
        assert!(pretty.contains("Top issues") || pretty.contains("Top issues (max 5):"));
    }

    #[test]
    fn format_pretty_handles_errors() {
        let output = DpcOutput::Error(ErrorOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            message: Some("bad input".to_string()),
            error: dpc_lib::error::ErrorPayload {
                category: dpc_lib::error::ErrorCategory::Config,
                message: "bad input".to_string(),
                remediation: Some("check flags".to_string()),
            },
        });

        let pretty = format_pretty(&output, false);
        assert!(pretty.contains("[ERROR] bad input"));
        assert!(pretty.contains("Hint: check flags"));
    }
}
