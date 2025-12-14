mod cli;

use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io;
use std::io::{BufWriter, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cli::{Commands, OutputFormat, ResourceType};
use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::types::{MetricScores, ResourceKind};
use dpc_lib::NormalizedView;
use dpc_lib::{
    calculate_combined_score, default_metrics, figma_to_normalized_view, image_to_normalized_view,
    parse_resource, run_metrics, url_to_normalized_view, CompareArtifacts, CompareOutput, DpcError,
    DpcOutput, ErrorOutput, FigmaClient, FigmaRenderOptions, FindingSeverity, GenerateCodeOutput,
    ImageLoadOptions, MetricKind, ParsedResource, QualityFinding, QualityOutput,
    ResourceDescriptor, ScoreWeights, Summary, UrlToViewOptions, Viewport,
};
use image::{self, imageops::FilterType, GenericImageView, RgbaImage};
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() -> ExitCode {
    run().await
}

async fn run() -> ExitCode {
    let args = cli::parse();

    match args.command {
        Commands::Compare {
            r#ref,
            r#impl,
            ref_type,
            impl_type,
            viewport,
            threshold,
            metrics,
            format,
            output,
            keep_artifacts,
            ignore_selectors,
            ignore_regions,
            artifacts_dir,
            nav_timeout,
            network_idle_timeout,
            process_timeout,
            ..
        } => {
            if args.verbose {
                eprintln!("Parsing resources…");
            }
            let ref_res = match parse_resource(&r#ref, ref_type.map(resource_kind_from_cli)) {
                Ok(res) => res,
                Err(err) => {
                    return render_error(DpcError::Config(err.to_string()), format, output.clone())
                }
            };
            let impl_res = match parse_resource(&r#impl, impl_type.map(resource_kind_from_cli)) {
                Ok(res) => res,
                Err(err) => {
                    return render_error(DpcError::Config(err.to_string()), format, output.clone())
                }
            };
            let selected_metrics = match parse_metric_kinds(metrics.as_deref()) {
                Ok(k) => k,
                Err(err) => {
                    return render_error(DpcError::Config(err.to_string()), format, output.clone())
                }
            };
            let ignore_selectors = parse_ignore_selectors(ignore_selectors.as_deref());
            let ignore_regions = match ignore_regions {
                Some(path) => match load_ignore_regions(&path) {
                    Ok(regions) => regions,
                    Err(err) => return render_error(err, format, output.clone()),
                },
                None => Vec::new(),
            };

            // Create temp directory for artifacts
            let (artifacts_dir, artifacts_from_cli) =
                resolve_artifacts_dir(artifacts_dir.as_deref());
            if let Err(err) = std::fs::create_dir_all(&artifacts_dir) {
                return render_error(DpcError::Io(err), format, output.clone());
            }
            let should_keep_artifacts = keep_artifacts || artifacts_from_cli;
            let progress_logger: Option<Arc<dyn Fn(&str) + Send + Sync>> = if args.verbose {
                Some(Arc::new(|msg: &str| eprintln!("{msg}")))
            } else {
                None
            };

            // Convert resources to NormalizedViews
            if args.verbose {
                eprintln!("Normalizing reference ({:?})…", ref_res.kind);
            }
            let ref_view_raw = match resource_to_normalized_view(
                &ref_res,
                &viewport,
                &artifacts_dir,
                "ref",
                progress_logger.clone(),
                nav_timeout,
                network_idle_timeout,
                process_timeout,
            )
            .await
            {
                Ok(view) => view,
                Err(err) => {
                    return render_error(
                        DpcError::Config(format!("Failed to process reference: {}", err)),
                        format,
                        output.clone(),
                    )
                }
            };

            if args.verbose {
                eprintln!("Normalizing implementation ({:?})…", impl_res.kind);
            }
            let impl_view_raw = match resource_to_normalized_view(
                &impl_res,
                &viewport,
                &artifacts_dir,
                "impl",
                progress_logger.clone(),
                nav_timeout,
                network_idle_timeout,
                process_timeout,
            )
            .await
            {
                Ok(view) => view,
                Err(err) => {
                    return render_error(
                        DpcError::Config(format!("Failed to process implementation: {}", err)),
                        format,
                        output.clone(),
                    )
                }
            };

            let ref_view = apply_dom_ignores(&ref_view_raw, &ignore_selectors);
            let impl_view = apply_dom_ignores(&impl_view_raw, &ignore_selectors);

            let ref_view = if ignore_regions.is_empty() {
                ref_view
            } else {
                match apply_ignore_regions(&ref_view, &ignore_regions, &artifacts_dir, "ref") {
                    Ok(view) => view,
                    Err(err) => return render_error(err, format, output.clone()),
                }
            };
            let impl_view = if ignore_regions.is_empty() {
                impl_view
            } else {
                match apply_ignore_regions(&impl_view, &ignore_regions, &artifacts_dir, "impl") {
                    Ok(view) => view,
                    Err(err) => return render_error(err, format, output.clone()),
                }
            };

            // Determine effective metrics based on input types
            // If no metrics specified and both inputs lack DOM data, use only image-compatible metrics
            let effective_metrics =
                if selected_metrics.is_empty() && ref_view.dom.is_none() && impl_view.dom.is_none()
                {
                    vec![MetricKind::Pixel, MetricKind::Color]
                } else {
                    selected_metrics
                };

            // Run metrics
            if args.verbose {
                eprintln!("Running metrics: {:?}", effective_metrics);
            }
            let all_metrics = default_metrics();
            let metrics_scores =
                match run_metrics(&all_metrics, &effective_metrics, &ref_view, &impl_view) {
                    Ok(scores) => scores,
                    Err(err) => {
                        return render_error(
                            DpcError::Config(format!("Failed to compute metrics: {}", err)),
                            format,
                            output.clone(),
                        )
                    }
                };

            // Calculate combined score
            let weights = ScoreWeights::default();
            let similarity = calculate_combined_score(&metrics_scores, &weights);

            // Determine pass/fail
            let passed = similarity >= threshold as f32;

            // Generate summary
            let summary = generate_summary(&metrics_scores, similarity, threshold as f32);

            let artifacts = if should_keep_artifacts {
                match persist_compare_artifacts(
                    &artifacts_dir,
                    &ref_view,
                    &impl_view,
                    should_keep_artifacts,
                ) {
                    Ok(paths) => Some(paths),
                    Err(err) => return render_error(err, format, output.clone()),
                }
            } else {
                None
            };

            if should_keep_artifacts {
                eprintln!("Artifacts saved to: {}", artifacts_dir.display());
            }

            if args.verbose {
                if let Some(paths) = &artifacts {
                    eprintln!(
                        "Artifacts directory: {} (kept: {})",
                        paths.directory.display(),
                        paths.kept
                    );
                    if let Some(path) = &paths.ref_screenshot {
                        eprintln!("  ref screenshot: {}", path.display());
                    }
                    if let Some(path) = &paths.impl_screenshot {
                        eprintln!("  impl screenshot: {}", path.display());
                    }
                    if let Some(path) = &paths.ref_dom_snapshot {
                        eprintln!("  ref DOM: {}", path.display());
                    }
                    if let Some(path) = &paths.impl_dom_snapshot {
                        eprintln!("  impl DOM: {}", path.display());
                    }
                    if let Some(path) = &paths.ref_figma_snapshot {
                        eprintln!("  ref figma tree: {}", path.display());
                    }
                    if let Some(path) = &paths.impl_figma_snapshot {
                        eprintln!("  impl figma tree: {}", path.display());
                    }
                    if paths.diff_image.is_some() {
                        if let Some(path) = &paths.diff_image {
                            eprintln!("  pixel diff: {}", path.display());
                        }
                    } else {
                        eprintln!("  pixel diff: not generated");
                    }
                    if !paths.kept {
                        eprintln!("Artifacts will be cleaned up; pass --keep-artifacts or --artifacts-dir to retain.");
                    }
                } else {
                    eprintln!(
                        "Artifacts directory: {} (will be cleaned up; use --keep-artifacts or --artifacts-dir to retain)",
                        artifacts_dir.display()
                    );
                }
            }

            let body = DpcOutput::Compare(CompareOutput {
                version: DPC_OUTPUT_VERSION.to_string(),
                ref_resource: ResourceDescriptor {
                    kind: ref_res.kind,
                    value: ref_res.value,
                },
                impl_resource: ResourceDescriptor {
                    kind: impl_res.kind,
                    value: impl_res.value,
                },
                viewport,
                similarity,
                threshold: threshold as f32,
                passed,
                metrics: metrics_scores,
                summary: Some(summary),
                artifacts,
            });

            if let Err(err) = write_output(&body, format, output.clone()) {
                return render_error(DpcError::Config(err.to_string()), format, output);
            }

            // Cleanup artifacts unless --keep-artifacts is set
            if !should_keep_artifacts {
                let _ = std::fs::remove_dir_all(&artifacts_dir);
            }

            exit_code_for_compare(passed)
        }
        Commands::GenerateCode {
            input,
            input_type,
            viewport,
            stack,
            output,
            format,
        } => {
            if args.verbose {
                eprintln!("Parsing input resource…");
            }
            let viewport = Some(viewport);
            let input_res = match parse_resource(&input, input_type.map(resource_kind_from_cli)) {
                Ok(res) => res,
                Err(err) => {
                    return render_error(DpcError::Config(err.to_string()), format, output.clone())
                }
            };
            if args.verbose {
                eprintln!(
                    "Normalized input ({:?}); generate-code is currently stubbed",
                    input_res.kind
                );
            }
            let body = DpcOutput::GenerateCode(GenerateCodeOutput {
                version: DPC_OUTPUT_VERSION.to_string(),
                input: ResourceDescriptor {
                    kind: input_res.kind,
                    value: input_res.value,
                },
                viewport,
                stack: Some(stack),
                output_path: output.clone(),
                code: None,
                summary: Some(Summary {
                    top_issues: vec![
                        "generate-code is not implemented yet; use an external screenshot-to-code service or mock outputs for now.".to_string(),
                    ],
                }),
            });
            if let Err(err) = write_output(&body, format, output.clone()) {
                return render_error(DpcError::Config(err.to_string()), format, output);
            }
            ExitCode::SUCCESS
        }
        Commands::Quality {
            input,
            input_type,
            viewport,
            format,
            output,
        } => {
            if args.verbose {
                eprintln!("Parsing input resource…");
            }
            let input_res = match parse_resource(&input, input_type.map(resource_kind_from_cli)) {
                Ok(res) => res,
                Err(err) => {
                    return render_error(DpcError::Config(err.to_string()), format, output.clone())
                }
            };
            if args.verbose {
                eprintln!(
                    "Computed normalized input ({:?}); quality mode is currently stubbed",
                    input_res.kind
                );
            }
            let body = DpcOutput::Quality(QualityOutput {
                version: DPC_OUTPUT_VERSION.to_string(),
                input: ResourceDescriptor {
                    kind: input_res.kind,
                    value: input_res.value,
                },
                viewport,
                score: 0.0,
                findings: vec![QualityFinding {
                    severity: FindingSeverity::Info,
                    finding_type: "not_implemented".to_string(),
                    message: "Quality mode is not implemented yet; run compare for parity checks or stub quality scoring.".to_string(),
                }],
            });
            if let Err(err) = write_output(&body, format, output.clone()) {
                return render_error(DpcError::Config(err.to_string()), format, output);
            }
            ExitCode::SUCCESS
        }
    }
}

async fn resource_to_normalized_view(
    resource: &ParsedResource,
    viewport: &Viewport,
    artifacts_dir: &Path,
    prefix: &str,
    progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    nav_timeout: u64,
    network_idle_timeout: u64,
    process_timeout: u64,
) -> Result<NormalizedView, Box<dyn std::error::Error + Send + Sync>> {
    if matches!(resource.kind, ResourceKind::Url | ResourceKind::Figma) {
        if let Some(mock_path) = mock_render_image_path(prefix) {
            let screenshot_path = artifacts_dir.join(format!("{}_screenshot.png", prefix));
            let options = ImageLoadOptions {
                no_resize: false,
                target_width: Some(viewport.width),
                target_height: Some(viewport.height),
            };
            let view = image_to_normalized_view(
                mock_path.as_str(),
                screenshot_path.to_string_lossy().as_ref(),
                options,
            )
            .map_err(|e| format!("Mock rendering failed: {}", e))?;
            return Ok(view);
        }
    }

    match resource.kind {
        ResourceKind::Image => {
            let screenshot_path = artifacts_dir.join(format!("{}_screenshot.png", prefix));
            let options = ImageLoadOptions {
                no_resize: false,
                target_width: Some(viewport.width),
                target_height: Some(viewport.height),
            };
            let view = image_to_normalized_view(
                resource.value.as_str(),
                &screenshot_path.to_string_lossy(),
                options,
            )
            .map_err(|e| format!("Image loading failed: {}", e))?;
            Ok(view)
        }
        ResourceKind::Url => {
            let screenshot_path = artifacts_dir.join(format!("{}_screenshot.png", prefix));
            let mut options = UrlToViewOptions::default();
            options.viewport = *viewport;
            options.progress = progress.clone();
            options.navigation_timeout = Duration::from_secs(nav_timeout);
            options.network_idle_timeout = Duration::from_secs(network_idle_timeout);
            options.process_timeout = Duration::from_secs(process_timeout);
            let view = url_to_normalized_view(resource.value.as_str(), &screenshot_path, options)
                .await
                .map_err(|e| format!("URL rendering failed: {}", e))?;
            Ok(view)
        }
        ResourceKind::Figma => {
            let figma_info = resource
                .figma_info
                .as_ref()
                .ok_or_else(|| DpcError::Config("Missing Figma file key".to_string()))?;
            let node_id = figma_info
                .node_id
                .clone()
                .ok_or_else(|| DpcError::Config("Figma node-id is required".to_string()))?;
            let token = std::env::var("FIGMA_TOKEN").map_err(|_| {
                DpcError::Config("FIGMA_TOKEN environment variable is required".to_string())
            })?;
            let client =
                FigmaClient::new(token).map_err(|e| format!("Figma client error: {}", e))?;
            let output_path = artifacts_dir.join(format!("{}_figma.png", prefix));
            let options = FigmaRenderOptions {
                file_key: figma_info.file_key.clone(),
                node_id,
                output_path,
                viewport: Some(*viewport),
                scale: 1.0,
            };
            let view = figma_to_normalized_view(&client, &options)
                .await
                .map_err(|e| format!("Figma rendering failed: {}", e))?;
            Ok(view)
        }
    }
}

fn mock_render_image_path(prefix: &str) -> Option<String> {
    let env_key = format!("DPC_MOCK_RENDER_{}", prefix.to_ascii_uppercase());
    if let Ok(path) = std::env::var(&env_key) {
        if !path.trim().is_empty() {
            return Some(path);
        }
    }

    if let Ok(dir) = std::env::var("DPC_MOCK_RENDERERS_DIR") {
        let candidate = std::path::Path::new(&dir).join(format!("{prefix}.png"));
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    None
}

#[derive(Debug, Clone, Deserialize)]
struct IgnoreRegion {
    pub x: f32,
    pub y: f32,
    #[serde(alias = "w")]
    pub width: f32,
    #[serde(alias = "h")]
    pub height: f32,
}

fn resolve_artifacts_dir(custom: Option<&Path>) -> (PathBuf, bool) {
    if let Some(dir) = custom {
        return (dir.to_path_buf(), true);
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("dpc-{}-{timestamp}", std::process::id()));
    (dir, false)
}

fn load_ignore_regions(path: &Path) -> Result<Vec<IgnoreRegion>, DpcError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| DpcError::Config(format!("Failed to read ignore-regions: {e}")))?;
    let regions: Vec<IgnoreRegion> = serde_json::from_str(&data).map_err(|e| {
        DpcError::Config(format!(
            "Invalid ignore-regions JSON (expected array of {{x,y,width,height}}; w/h aliases allowed): {e}"
        ))
    })?;

    if regions.is_empty() {
        return Err(DpcError::Config(
            "ignore-regions file contained no regions".to_string(),
        ));
    }

    Ok(regions)
}

fn apply_ignore_regions(
    view: &NormalizedView,
    regions: &[IgnoreRegion],
    artifacts_dir: &Path,
    prefix: &str,
) -> Result<NormalizedView, DpcError> {
    if regions.is_empty() {
        return Ok(view.clone());
    }

    let mut image = image::open(&view.screenshot_path)
        .map_err(DpcError::from)?
        .to_rgba8();
    let (img_w, img_h) = image.dimensions();

    for region in regions {
        if region.width <= 0.0 || region.height <= 0.0 {
            continue;
        }
        let use_normalized = region.x >= 0.0
            && region.y >= 0.0
            && region.x <= 1.0
            && region.y <= 1.0
            && region.width <= 1.0
            && region.height <= 1.0;
        let (rx, ry, rw, rh) = if use_normalized {
            (
                region.x * img_w as f32,
                region.y * img_h as f32,
                region.width * img_w as f32,
                region.height * img_h as f32,
            )
        } else {
            (region.x, region.y, region.width, region.height)
        };

        let x0 = rx.max(0.0).floor() as u32;
        let y0 = ry.max(0.0).floor() as u32;
        let x1 = (rx + rw).ceil().max(0.0) as u32;
        let y1 = (ry + rh).ceil().max(0.0) as u32;

        let x_start = x0.min(img_w);
        let y_start = y0.min(img_h);
        let x_end = x1.min(img_w);
        let y_end = y1.min(img_h);

        for y in y_start..y_end {
            for x in x_start..x_end {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
    }

    let masked_path = artifacts_dir.join(format!("{prefix}_masked.png"));
    image
        .save(&masked_path)
        .map_err(|e| DpcError::Config(format!("Failed to save masked screenshot: {e}")))?;

    let mut updated = view.clone();
    updated.screenshot_path = masked_path;
    Ok(updated)
}

fn generate_diff_heatmap(
    ref_path: &Path,
    impl_path: &Path,
    output_path: &Path,
) -> Result<(), DpcError> {
    let ref_img = image::open(ref_path).map_err(DpcError::from)?;
    let mut impl_img = image::open(impl_path).map_err(DpcError::from)?;

    let (ref_w, ref_h) = ref_img.dimensions();
    let (impl_w, impl_h) = impl_img.dimensions();
    if (impl_w, impl_h) != (ref_w, ref_h) {
        impl_img = impl_img.resize_exact(ref_w, ref_h, FilterType::Lanczos3);
    }

    let ref_rgba = ref_img.to_rgba8();
    let impl_rgba = impl_img.to_rgba8();
    let mut heat = RgbaImage::new(ref_w, ref_h);

    for y in 0..ref_h {
        for x in 0..ref_w {
            let p_ref = ref_rgba.get_pixel(x, y);
            let p_impl = impl_rgba.get_pixel(x, y);
            let diff = (p_ref[0] as i16 - p_impl[0] as i16).abs()
                + (p_ref[1] as i16 - p_impl[1] as i16).abs()
                + (p_ref[2] as i16 - p_impl[2] as i16).abs();
            let ratio = (diff as f32 / 765.0).clamp(0.0, 1.0);
            let alpha = (ratio * 200.0).clamp(0.0, 200.0) as u8;

            // Color coding: green (minor), yellow (moderate), red (major)
            let pixel = if ratio < 0.33 {
                let g = (100.0 + ratio / 0.33 * 100.0).clamp(0.0, 200.0) as u8;
                image::Rgba([0, g, 0, alpha])
            } else if ratio < 0.66 {
                let g = 180u8;
                let r = (150.0 + (ratio - 0.33) / 0.33 * 80.0).clamp(150.0, 230.0) as u8;
                image::Rgba([r, g, 0, alpha])
            } else {
                let r = (200.0 + (ratio - 0.66) / 0.34 * 55.0).clamp(200.0, 255.0) as u8;
                image::Rgba([r, 0, 0, alpha])
            };
            heat.put_pixel(x, y, pixel);
        }
    }

    heat.save(output_path)
        .map_err(|e| DpcError::Config(format!("Failed to save diff heatmap: {e}")))?;

    Ok(())
}

fn persist_compare_artifacts(
    artifacts_dir: &Path,
    ref_view: &NormalizedView,
    impl_view: &NormalizedView,
    keep: bool,
) -> Result<CompareArtifacts, DpcError> {
    let mut artifacts = CompareArtifacts {
        directory: artifacts_dir.to_path_buf(),
        kept: keep,
        ref_screenshot: Some(ref_view.screenshot_path.clone()),
        impl_screenshot: Some(impl_view.screenshot_path.clone()),
        diff_image: None,
        ref_dom_snapshot: None,
        impl_dom_snapshot: None,
        ref_figma_snapshot: None,
        impl_figma_snapshot: None,
    };

    if keep {
        // Save diff heatmap for quick visual inspection
        let diff_path = artifacts_dir.join("diff_heatmap.png");
        generate_diff_heatmap(
            &ref_view.screenshot_path,
            &impl_view.screenshot_path,
            &diff_path,
        )?;
        artifacts.diff_image = Some(diff_path);

        if let Some(dom) = &ref_view.dom {
            let path = artifacts_dir.join("ref_dom.json");
            write_json_pretty(&path, dom)?;
            artifacts.ref_dom_snapshot = Some(path);
        }

        if let Some(dom) = &impl_view.dom {
            let path = artifacts_dir.join("impl_dom.json");
            write_json_pretty(&path, dom)?;
            artifacts.impl_dom_snapshot = Some(path);
        }

        if let Some(figma_tree) = &ref_view.figma_tree {
            let path = artifacts_dir.join("ref_figma.json");
            write_json_pretty(&path, figma_tree)?;
            artifacts.ref_figma_snapshot = Some(path);
        }

        if let Some(figma_tree) = &impl_view.figma_tree {
            let path = artifacts_dir.join("impl_figma.json");
            write_json_pretty(&path, figma_tree)?;
            artifacts.impl_figma_snapshot = Some(path);
        }
    }

    Ok(artifacts)
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), DpcError> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)?;
    Ok(())
}

fn generate_summary(scores: &MetricScores, similarity: f32, threshold: f32) -> Summary {
    let mut top_issues = Vec::new();

    // Check each metric and generate human-readable issues
    if let Some(ref pixel) = scores.pixel {
        if pixel.score < 0.9 {
            let diff_pct = ((1.0 - pixel.score) * 100.0).round();
            top_issues.push(format!(
                "Pixel differences detected in ~{}% of the image",
                diff_pct
            ));
        }
        if !pixel.diff_regions.is_empty() {
            let major_regions = pixel
                .diff_regions
                .iter()
                .filter(|r| matches!(r.severity, dpc_lib::types::DiffSeverity::Major))
                .count();
            if major_regions > 0 {
                top_issues.push(format!(
                    "{} major visual difference region(s) found",
                    major_regions
                ));
            }
        }
    }

    if let Some(ref layout) = scores.layout {
        if layout.score < 0.9 {
            let missing = layout
                .diff_regions
                .iter()
                .filter(|r| matches!(r.kind, dpc_lib::types::LayoutDiffKind::MissingElement))
                .count();
            let extra = layout
                .diff_regions
                .iter()
                .filter(|r| matches!(r.kind, dpc_lib::types::LayoutDiffKind::ExtraElement))
                .count();
            let shifted = layout
                .diff_regions
                .iter()
                .filter(|r| matches!(r.kind, dpc_lib::types::LayoutDiffKind::PositionShift))
                .count();

            if missing > 0 {
                top_issues.push(format!(
                    "{} element(s) missing from implementation",
                    missing
                ));
            }
            if extra > 0 {
                top_issues.push(format!("{} extra element(s) in implementation", extra));
            }
            if shifted > 0 {
                top_issues.push(format!(
                    "{} element(s) shifted from expected position",
                    shifted
                ));
            }
        }
    }

    if let Some(ref typo) = scores.typography {
        if typo.score < 0.9 && !typo.diffs.is_empty() {
            let font_issues = typo
                .diffs
                .iter()
                .filter(|d| {
                    d.issues
                        .iter()
                        .any(|i| matches!(i, dpc_lib::types::TypographyIssue::FontFamilyMismatch))
                })
                .count();
            let size_issues = typo
                .diffs
                .iter()
                .filter(|d| {
                    d.issues
                        .iter()
                        .any(|i| matches!(i, dpc_lib::types::TypographyIssue::FontSizeDiff))
                })
                .count();

            if font_issues > 0 {
                top_issues.push(format!(
                    "{} element(s) have mismatched font families",
                    font_issues
                ));
            }
            if size_issues > 0 {
                top_issues.push(format!(
                    "{} element(s) have incorrect font sizes",
                    size_issues
                ));
            }
        }
    }

    if let Some(ref color) = scores.color {
        if color.score < 0.9 && !color.diffs.is_empty() {
            top_issues.push(format!(
                "{} color difference(s) detected in palette",
                color.diffs.len()
            ));
        }
    }

    if let Some(ref content) = scores.content {
        if content.score < 0.9 {
            if !content.missing_text.is_empty() {
                top_issues.push(format!(
                    "{} text element(s) missing from implementation",
                    content.missing_text.len()
                ));
            }
            if !content.extra_text.is_empty() {
                top_issues.push(format!(
                    "{} extra text element(s) in implementation",
                    content.extra_text.len()
                ));
            }
        }
    }

    // Add overall status
    if similarity >= threshold {
        top_issues.insert(
            0,
            format!(
                "Design parity check passed ({:.1}% similarity, threshold: {:.1}%)",
                similarity * 100.0,
                threshold * 100.0
            ),
        );
    } else {
        top_issues.insert(
            0,
            format!(
                "Design parity check failed ({:.1}% similarity, threshold: {:.1}%)",
                similarity * 100.0,
                threshold * 100.0
            ),
        );
    }

    Summary { top_issues }
}

fn resource_kind_from_cli(rt: ResourceType) -> ResourceKind {
    match rt {
        ResourceType::Url => ResourceKind::Url,
        ResourceType::Image => ResourceKind::Image,
        ResourceType::Figma => ResourceKind::Figma,
    }
}

fn parse_metric_kinds(
    kinds: Option<&[String]>,
) -> Result<Vec<MetricKind>, Box<dyn std::error::Error>> {
    let mut parsed = Vec::new();
    if let Some(items) = kinds {
        for item in items {
            let kind = MetricKind::from_str(item).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid metric kind '{}': {}", item, e),
                )
            })?;
            parsed.push(kind);
        }
    }
    Ok(parsed)
}

fn parse_ignore_selectors(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .filter_map(|part| {
                let trimmed = part.trim().to_ascii_lowercase();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

fn apply_dom_ignores(view: &NormalizedView, selectors: &[String]) -> NormalizedView {
    if selectors.is_empty() {
        return view.clone();
    }

    let mut filtered = view.clone();
    if let Some(dom) = &view.dom {
        let nodes = dom
            .nodes
            .iter()
            .filter(|n| !matches_any_selector(n, selectors))
            .cloned()
            .collect();
        let mut dom_filtered = dom.clone();
        dom_filtered.nodes = nodes;
        filtered.dom = Some(dom_filtered);
    }
    filtered
}

fn matches_any_selector(node: &dpc_lib::types::DomNode, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| selector_matches(node, sel))
}

fn selector_matches(node: &dpc_lib::types::DomNode, selector: &str) -> bool {
    if let Some(id) = selector.strip_prefix('#') {
        let id = id.to_ascii_lowercase();
        let attr_id = node
            .attributes
            .get("id")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        let node_id = node.id.to_ascii_lowercase();
        return attr_id == id || node_id == id;
    }

    if let Some(class) = selector.strip_prefix('.') {
        let class = class.to_ascii_lowercase();
        if let Some(attr) = node.attributes.get("class") {
            let has = attr
                .split_whitespace()
                .any(|c| c.eq_ignore_ascii_case(&class));
            if has {
                return true;
            }
        }
        return false;
    }

    node.tag.eq_ignore_ascii_case(selector)
}

fn write_output(
    body: &DpcOutput,
    format: OutputFormat,
    output: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Json => write_json_output(body, output.as_deref())?,
        OutputFormat::Pretty => write_pretty_output(body, output.as_deref())?,
    };
    Ok(())
}

fn render_error(err: DpcError, format: OutputFormat, output: Option<PathBuf>) -> ExitCode {
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

fn write_json_output(body: &DpcOutput, output: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let content = serde_json::to_string(body)?;
    if let Some(path) = output {
        std::fs::write(path, content)?;
    } else {
        println!("{content}");
    }
    Ok(())
}

fn write_pretty_output(body: &DpcOutput, output: Option<&Path>) -> io::Result<()> {
    let content = format_pretty_plain(body);
    if let Some(path) = output {
        std::fs::write(path, &content)?;
    } else {
        println!("{content}");
    }
    Ok(())
}

fn format_pretty(body: &DpcOutput, colorize: bool) -> String {
    match body {
        DpcOutput::Compare(out) => {
            let mut buf = String::new();
            let status = if out.passed { "PASS" } else { "FAIL" };
            let status_colored = color(status, if out.passed { "32" } else { "31" }, colorize);
            let similarity = format!("{:.1}%", out.similarity * 100.0);
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

            let mut metrics = Vec::new();
            if let Some(pixel) = &out.metrics.pixel {
                metrics.push(("pixel", format!("{:.3}", pixel.score)));
            }
            if let Some(layout) = &out.metrics.layout {
                metrics.push(("layout", format!("{:.3}", layout.score)));
            }
            if let Some(typography) = &out.metrics.typography {
                metrics.push(("typography", format!("{:.3}", typography.score)));
            }
            if let Some(color_metric) = &out.metrics.color {
                metrics.push(("color", format!("{:.3}", color_metric.score)));
            }
            if let Some(content) = &out.metrics.content {
                metrics.push(("content", format!("{:.3}", content.score)));
            }
            if !metrics.is_empty() {
                writeln!(buf, "Metrics:").ok();
                for (name, score) in metrics {
                    writeln!(buf, "- {:12} {}", name, score).ok();
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
                }
            }

            buf
        }
        DpcOutput::GenerateCode(out) => {
            let mut buf = String::new();
            let header = color("[GENERATE]", "36", colorize);
            writeln!(buf, "{} Code generation (stub)", header).ok();
            writeln!(
                buf,
                "Input: {} (kind: {:?})",
                out.input.value, out.input.kind
            )
            .ok();
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
                    writeln!(buf, "- [{:?}] {}", finding.severity, finding.message).ok();
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

fn color(text: &str, code: &str, colorize: bool) -> String {
    if colorize {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

fn format_pretty_plain(body: &DpcOutput) -> String {
    serde_json::to_string(body).unwrap_or_else(|_| "{\"mode\":\"error\"}".to_string())
}

fn exit_code_for_compare(passed: bool) -> ExitCode {
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpc_lib::types::{BoundingBox, DomNode, DomSnapshot};
    use std::collections::HashMap;

    fn make_node(id: &str, tag: &str, class: Option<&str>) -> DomNode {
        let mut attrs = HashMap::new();
        if let Some(class) = class {
            attrs.insert("class".to_string(), class.to_string());
        }
        DomNode {
            id: id.to_string(),
            tag: tag.to_string(),
            children: vec![],
            parent: None,
            attributes: attrs,
            text: None,
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            computed_style: None,
        }
    }

    fn view_with_dom(nodes: Vec<DomNode>) -> NormalizedView {
        NormalizedView {
            kind: ResourceKind::Url,
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
    fn parse_ignore_selectors_normalizes_and_trims() {
        let parsed = parse_ignore_selectors(Some("  #Hero , .Ad ,p  ,, "));
        assert_eq!(parsed, vec!["#hero", ".ad", "p"]);
    }

    #[test]
    fn apply_dom_ignores_filters_on_id_class_and_tag() {
        let nodes = vec![
            make_node("hero", "div", Some("banner")),
            make_node("ad1", "div", Some("ad slot")),
            make_node("p1", "p", None),
        ];
        let view = view_with_dom(nodes);
        let selectors = vec!["#ad1".to_string(), ".banner".to_string(), "p".to_string()];
        let filtered = apply_dom_ignores(&view, &selectors);

        let kept: Vec<String> = filtered
            .dom
            .unwrap()
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .collect();
        assert!(kept.is_empty(), "all nodes should be ignored");
    }

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
    fn generate_diff_heatmap_creates_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ref_path = tmp.path().join("ref.png");
        let impl_path = tmp.path().join("impl.png");
        let out_path = tmp.path().join("diff_heatmap.png");

        let ref_img = RgbaImage::from_pixel(2, 2, image::Rgba([10, 10, 10, 255]));
        let impl_img = RgbaImage::from_pixel(2, 2, image::Rgba([200, 200, 200, 255]));
        ref_img.save(&ref_path).unwrap();
        impl_img.save(&impl_path).unwrap();

        generate_diff_heatmap(&ref_path, &impl_path, &out_path).unwrap();
        assert!(out_path.exists(), "heatmap file should be created");
        let meta = std::fs::metadata(&out_path).unwrap();
        assert!(meta.len() > 0, "heatmap should not be empty");
    }

    #[test]
    fn format_pretty_plain_includes_status_and_metrics() {
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
                pixel: Some(dpc_lib::types::PixelMetric {
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

        let pretty = format_pretty_plain(&output);
        let parsed: serde_json::Value =
            serde_json::from_str(&pretty).expect("pretty output should be JSON");
        assert_eq!(parsed.get("mode").and_then(|v| v.as_str()), Some("compare"));
        assert_eq!(parsed.get("passed").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            parsed
                .get("metrics")
                .and_then(|m| m.get("pixel"))
                .and_then(|p| p.get("score"))
                .and_then(|s| s.as_f64()),
            Some(0.96)
        );
        let top_issues = parsed
            .get("summary")
            .and_then(|s| s.get("topIssues"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !top_issues.is_empty(),
            "expected top issues to be included"
        );
    }

    #[test]
    fn format_pretty_plain_handles_errors() {
        let output = DpcOutput::Error(ErrorOutput {
            version: DPC_OUTPUT_VERSION.to_string(),
            message: Some("bad input".to_string()),
            error: dpc_lib::error::ErrorPayload {
                category: dpc_lib::error::ErrorCategory::Config,
                message: "bad input".to_string(),
                remediation: Some("check flags".to_string()),
            },
        });

        let pretty = format_pretty_plain(&output);
        let parsed: serde_json::Value =
            serde_json::from_str(&pretty).expect("pretty output should be JSON");
        assert_eq!(parsed.get("mode").and_then(|v| v.as_str()), Some("error"));
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str()),
            Some("bad input")
        );
        let remediation = parsed
            .get("error")
            .and_then(|e| e.get("remediation"))
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string();
        assert!(
            remediation.contains("check flags"),
            "should include remediation hint, got {remediation}"
        );
    }
}
