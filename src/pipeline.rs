use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::{imageops::FilterType, GenericImageView, RgbaImage};
use serde::{Deserialize, Serialize};

use dpc_lib::types::{DomNode, MetricScores, ResourceKind, Viewport};
use dpc_lib::{
    figma_to_normalized_view, image_to_normalized_view, url_to_normalized_view,
    CompareArtifacts, DpcError, FigmaAuth, FigmaClient, FigmaRenderOptions,
    ImageLoadOptions, NormalizedView, ParsedResource, Summary, UrlToViewOptions,
};

/// Convert a parsed resource to a NormalizedView.
pub async fn resource_to_normalized_view(
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
            let auth = FigmaAuth::from_env().ok_or_else(|| {
                DpcError::Config(
                    "Figma token missing; set FIGMA_TOKEN or FIGMA_OAUTH_TOKEN".to_string(),
                )
            })?;
            let client =
                FigmaClient::from_auth(auth).map_err(|e| format!("Figma client error: {}", e))?;
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

/// Check for mock render image path from environment variables.
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

/// Ignore region for masking areas in images.
#[derive(Debug, Clone, Deserialize)]
pub struct IgnoreRegion {
    pub x: f32,
    pub y: f32,
    #[serde(alias = "w")]
    pub width: f32,
    #[serde(alias = "h")]
    pub height: f32,
}

/// Resolve artifacts directory path.
pub fn resolve_artifacts_dir(custom: Option<&Path>) -> (PathBuf, bool) {
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

/// Load ignore regions from a JSON file.
pub fn load_ignore_regions(path: &Path) -> Result<Vec<IgnoreRegion>, DpcError> {
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

/// Apply ignore regions by masking areas in the screenshot.
pub fn apply_ignore_regions(
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

/// Parse ignore selectors from comma-separated string.
pub fn parse_ignore_selectors(raw: Option<&str>) -> Vec<String> {
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

/// Apply DOM ignores by filtering nodes matching selectors.
pub fn apply_dom_ignores(view: &NormalizedView, selectors: &[String]) -> NormalizedView {
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

fn matches_any_selector(node: &DomNode, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| selector_matches(node, sel))
}

fn selector_matches(node: &DomNode, selector: &str) -> bool {
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

/// Generate diff heatmap image from two screenshots.
pub fn generate_diff_heatmap(
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

/// Persist compare artifacts to disk.
pub fn persist_compare_artifacts(
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

/// Generate summary of metric scores.
pub fn generate_summary(scores: &MetricScores, similarity: f32, threshold: f32) -> Summary {
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

#[cfg(test)]
mod tests {
    use super::*;
    use dpc_lib::types::{BoundingBox, DomSnapshot, ResourceKind};
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
}
