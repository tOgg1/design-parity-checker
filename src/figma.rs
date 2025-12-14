use crate::figma_client::FigmaAuth;
use crate::types::{
    BoundingBox, FigmaNode, FigmaPaint, FigmaPaintKind, FigmaSnapshot, NormalizedView,
    ResourceKind, TypographyStyle,
};
use crate::{image_loader::resize_with_letterbox, DpcError, Result, Viewport};
use image::{load_from_memory, DynamicImage, GenericImageView};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FigmaError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Figma API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("Missing access token")]
    MissingToken,
    #[error("Invalid file key: {0}")]
    InvalidFileKey(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),
}

#[derive(Debug)]
pub struct FigmaClient {
    client: reqwest::Client,
    access_token: String,
    base_url: String,
}

fn map_figma_error(e: FigmaError) -> DpcError {
    match e {
        FigmaError::Request(req_err) => DpcError::Network(req_err),
        FigmaError::Api { status, message } => DpcError::FigmaApi {
            status: Some(
                reqwest::StatusCode::from_u16(status)
                    .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            ),
            message,
        },
        FigmaError::MissingToken => DpcError::Config(
            "Missing Figma token; set FIGMA_TOKEN or FIGMA_OAUTH_TOKEN".to_string(),
        ),
        FigmaError::InvalidFileKey(key) => DpcError::FigmaApi {
            status: None,
            message: format!("Invalid Figma file key: {}", key),
        },
        FigmaError::NodeNotFound(id) => DpcError::FigmaApi {
            status: None,
            message: format!("Node not found: {}", id),
        },
        FigmaError::RateLimited(secs) => DpcError::FigmaApi {
            status: Some(reqwest::StatusCode::TOO_MANY_REQUESTS),
            message: format!("Rate limited, retry after {} seconds", secs),
        },
    }
}

impl FigmaClient {
    pub fn new(access_token: impl Into<String>) -> std::result::Result<Self, FigmaError> {
        Self::from_auth(FigmaAuth::PersonalAccessToken(access_token.into()))
    }

    pub fn from_auth(auth: FigmaAuth) -> std::result::Result<Self, FigmaError> {
        Self::with_base_url(auth, "https://api.figma.com/v1")
    }

    pub fn with_base_url(
        auth: FigmaAuth,
        base_url: impl Into<String>,
    ) -> std::result::Result<Self, FigmaError> {
        let token = match &auth {
            FigmaAuth::PersonalAccessToken(token) | FigmaAuth::OAuthToken(token) => token.clone(),
        };

        if token.is_empty() {
            return Err(FigmaError::MissingToken);
        }

        let mut headers = HeaderMap::new();
        match auth {
            FigmaAuth::PersonalAccessToken(token) => {
                headers.insert(
                    reqwest::header::HeaderName::from_static("x-figma-token"),
                    HeaderValue::from_str(&token).map_err(|_| FigmaError::MissingToken)?,
                );
            }
            FigmaAuth::OAuthToken(token) => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", token))
                        .map_err(|_| FigmaError::MissingToken)?,
                );
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .no_proxy()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            access_token: token,
            base_url: base_url.into(),
        })
    }

    pub async fn get_file(&self, file_key: &str) -> std::result::Result<FigmaFile, FigmaError> {
        let url = format!("{}/files/{}", self.base_url, file_key);
        let response = self.client.get(&url).send().await?;

        self.handle_response(response).await
    }

    pub async fn get_file_nodes(
        &self,
        file_key: &str,
        node_ids: &[&str],
    ) -> std::result::Result<FigmaNodesResponse, FigmaError> {
        let ids = node_ids.join(",");
        let url = format!("{}/files/{}/nodes?ids={}", self.base_url, file_key, ids);
        let response = self.client.get(&url).send().await?;

        self.handle_response(response).await
    }

    pub async fn export_image(
        &self,
        file_key: &str,
        node_id: &str,
        format: ImageFormat,
        scale: f32,
    ) -> std::result::Result<String, FigmaError> {
        let url = format!(
            "{}/images/{}?ids={}&format={}&scale={}",
            self.base_url,
            file_key,
            node_id,
            format.as_str(),
            scale
        );

        let response = self.client.get(&url).send().await?;
        let export: FigmaImageExport = self.handle_response(response).await?;

        export
            .images
            .get(node_id)
            .cloned()
            .ok_or_else(|| FigmaError::NodeNotFound(node_id.to_string()))
    }

    pub async fn download_image(&self, url: &str) -> std::result::Result<Vec<u8>, FigmaError> {
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(FigmaError::Api {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
    ) -> std::result::Result<T, FigmaError> {
        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            return Err(FigmaError::RateLimited(retry_after));
        }

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(FigmaError::Api {
                status: status.as_u16(),
                message,
            });
        }

        Ok(response.json().await?)
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }
}

#[derive(Debug, Clone)]
pub struct FigmaRenderOptions {
    pub file_key: String,
    pub node_id: String,
    pub output_path: PathBuf,
    pub viewport: Option<Viewport>,
    pub scale: f32,
}

impl Default for FigmaRenderOptions {
    fn default() -> Self {
        Self {
            file_key: String::new(),
            node_id: String::new(),
            output_path: PathBuf::new(),
            viewport: None,
            scale: 1.0,
        }
    }
}

pub async fn figma_to_normalized_view(
    client: &FigmaClient,
    options: &FigmaRenderOptions,
) -> Result<NormalizedView> {
    if options.scale <= 0.0 {
        return Err(DpcError::Config(
            "Figma export scale must be greater than zero".to_string(),
        ));
    }
    if options.file_key.trim().is_empty() {
        return Err(DpcError::Config(
            "Figma file key is required for export".to_string(),
        ));
    }
    if options.node_id.trim().is_empty() {
        return Err(DpcError::Config(
            "Figma node id is required for export".to_string(),
        ));
    }
    if options.output_path.as_os_str().is_empty() {
        return Err(DpcError::Config(
            "Figma export output_path is required".to_string(),
        ));
    }

    let nodes_response = client
        .get_file_nodes(&options.file_key, &[&options.node_id])
        .await
        .map_err(map_figma_error)?;

    let node = nodes_response
        .nodes
        .get(&options.node_id)
        .ok_or_else(|| DpcError::FigmaApi {
            status: None,
            message: format!("Node {} not found in Figma response", options.node_id),
        })?;

    let figma_snapshot = build_figma_snapshot(&options.file_key, &options.node_id, &node.document);

    let image_url = client
        .export_image(
            &options.file_key,
            &options.node_id,
            ImageFormat::Png,
            options.scale,
        )
        .await
        .map_err(map_figma_error)?;

    let bytes = client
        .download_image(&image_url)
        .await
        .map_err(map_figma_error)?;

    let decoded_image = load_from_memory(&bytes)?;
    let source_dimensions = decoded_image.dimensions();
    let (width, height, letterbox) =
        finalize_figma_image(decoded_image, &options.output_path, options.viewport)?;

    let root_bb = node
        .document
        .absolute_bounding_box
        .as_ref()
        .map(|bb| map_bounding_box(Some(bb)));
    let figma_snapshot =
        normalize_figma_snapshot(figma_snapshot, root_bb, source_dimensions, &letterbox);

    Ok(NormalizedView {
        kind: ResourceKind::Figma,
        screenshot_path: options.output_path.clone(),
        width,
        height,
        dom: None,
        figma_tree: Some(figma_snapshot),
        ocr_blocks: None,
    })
}

#[derive(Debug, Clone, Copy)]
pub enum ImageFormat {
    Png,
    Jpg,
    Svg,
    Pdf,
}

impl ImageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpg => "jpg",
            ImageFormat::Svg => "svg",
            ImageFormat::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaFile {
    pub name: String,
    pub last_modified: String,
    pub version: String,
    pub document: FigmaDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaDocument {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub children: Vec<FigmaNodeData>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaNodeData {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub children: Vec<FigmaNodeData>,
    pub absolute_bounding_box: Option<FigmaBoundingBox>,
    pub characters: Option<String>,
    pub style: Option<FigmaTypeStyle>,
    #[serde(default)]
    pub fills: Vec<FigmaPaintData>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaBoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaTypeStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<f32>,
    pub line_height_px: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaPaintData {
    #[serde(rename = "type")]
    pub paint_type: String,
    pub color: Option<FigmaColor>,
    pub opacity: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl FigmaColor {
    pub fn to_hex(&self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct FigmaNodesResponse {
    pub nodes: HashMap<String, FigmaNodeWrapper>,
}

#[derive(Debug, Deserialize)]
pub struct FigmaNodeWrapper {
    pub document: FigmaNodeData,
}

#[derive(Debug, Deserialize)]
pub struct FigmaImageExport {
    pub images: HashMap<String, String>,
}

fn build_figma_snapshot(file_key: &str, node_id: &str, root: &FigmaNodeData) -> FigmaSnapshot {
    let mut nodes = Vec::new();
    collect_figma_nodes(root, &mut nodes);

    FigmaSnapshot {
        file_key: file_key.to_string(),
        node_id: node_id.to_string(),
        name: Some(root.name.clone()),
        nodes,
    }
}

fn collect_figma_nodes(node: &FigmaNodeData, acc: &mut Vec<FigmaNode>) {
    let children_ids: Vec<String> = node.children.iter().map(|c| c.id.clone()).collect();
    for child in &node.children {
        collect_figma_nodes(child, acc);
    }

    acc.push(FigmaNode {
        id: node.id.clone(),
        name: Some(node.name.clone()),
        node_type: node.node_type.clone(),
        bounding_box: map_bounding_box(node.absolute_bounding_box.as_ref()),
        text: node.characters.clone(),
        typography: node.style.as_ref().map(map_typography),
        fills: node.fills.iter().filter_map(map_paint).collect(),
        children: children_ids,
    });
}

fn map_typography(style: &FigmaTypeStyle) -> TypographyStyle {
    TypographyStyle {
        font_family: style.font_family.clone(),
        font_size: style.font_size,
        font_weight: style.font_weight.map(|w| w.to_string()),
        line_height: style.line_height_px,
    }
}

fn map_bounding_box(bb: Option<&FigmaBoundingBox>) -> BoundingBox {
    bb.map(|b| BoundingBox {
        x: b.x,
        y: b.y,
        width: b.width,
        height: b.height,
    })
    .unwrap_or(BoundingBox {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    })
}

fn map_paint(paint: &FigmaPaintData) -> Option<FigmaPaint> {
    let kind = match paint.paint_type.to_lowercase().as_str() {
        "solid" => FigmaPaintKind::Solid,
        "image" => FigmaPaintKind::Image,
        v if v.starts_with("gradient") => FigmaPaintKind::Gradient,
        _ => FigmaPaintKind::Solid,
    };

    Some(FigmaPaint {
        kind,
        color: paint.color.as_ref().map(|c| c.to_hex()),
        opacity: paint.opacity,
    })
}

#[derive(Debug, Clone, Copy)]
struct LetterboxTransform {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

fn compute_letterbox_transform(
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> LetterboxTransform {
    let scale_w = target_width as f64 / source_width as f64;
    let scale_h = target_height as f64 / source_height as f64;
    let scale = scale_w.min(scale_h);
    let new_w = (source_width as f64 * scale).round() as u32;
    let new_h = (source_height as f64 * scale).round() as u32;
    let offset_x = ((target_width as i64 - new_w as i64) / 2) as f32;
    let offset_y = ((target_height as i64 - new_h as i64) / 2) as f32;

    LetterboxTransform {
        scale: scale as f32,
        offset_x,
        offset_y,
    }
}

fn finalize_figma_image(
    img: DynamicImage,
    output_path: &std::path::Path,
    viewport: Option<Viewport>,
) -> Result<(u32, u32, LetterboxTransform)> {
    let (source_width, source_height) = img.dimensions();
    let (target_width, target_height) = viewport
        .map(|vp| (vp.width, vp.height))
        .unwrap_or((source_width, source_height));

    let letterbox =
        compute_letterbox_transform(source_width, source_height, target_width, target_height);

    let final_img = if viewport.is_some() {
        resize_with_letterbox(&img, target_width, target_height)
    } else {
        img
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    final_img.save(output_path)?;

    Ok((target_width, target_height, letterbox))
}

fn normalize_figma_snapshot(
    snapshot: FigmaSnapshot,
    root_bb: Option<BoundingBox>,
    source_dimensions: (u32, u32),
    letterbox: &LetterboxTransform,
) -> FigmaSnapshot {
    let (source_w, source_h) = source_dimensions;
    let (root_x, root_y, root_w, root_h) = root_bb
        .map(|bb| (bb.x, bb.y, bb.width, bb.height))
        .unwrap_or((0.0, 0.0, source_w as f32, source_h as f32));

    let scale_x = if root_w > 0.0 {
        source_w as f32 / root_w
    } else {
        1.0
    };
    let scale_y = if root_h > 0.0 {
        source_h as f32 / root_h
    } else {
        1.0
    };

    let mut nodes = Vec::with_capacity(snapshot.nodes.len());
    for node in snapshot.nodes {
        let rel_x = node.bounding_box.x - root_x;
        let rel_y = node.bounding_box.y - root_y;

        let scaled_x = rel_x * scale_x;
        let scaled_y = rel_y * scale_y;
        let scaled_w = node.bounding_box.width * scale_x;
        let scaled_h = node.bounding_box.height * scale_y;

        let final_x = scaled_x * letterbox.scale + letterbox.offset_x;
        let final_y = scaled_y * letterbox.scale + letterbox.offset_y;
        let final_w = scaled_w * letterbox.scale;
        let final_h = scaled_h * letterbox.scale;

        nodes.push(FigmaNode {
            bounding_box: BoundingBox {
                x: final_x,
                y: final_y,
                width: final_w,
                height: final_h,
            },
            ..node
        });
    }

    FigmaSnapshot { nodes, ..snapshot }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};
    use tempfile::TempDir;

    #[test]
    fn test_figma_client_missing_token() {
        let result = FigmaClient::new("");
        assert!(matches!(result.unwrap_err(), FigmaError::MissingToken));
    }

    #[test]
    fn test_image_format_as_str() {
        assert_eq!(ImageFormat::Png.as_str(), "png");
        assert_eq!(ImageFormat::Jpg.as_str(), "jpg");
        assert_eq!(ImageFormat::Svg.as_str(), "svg");
    }

    #[test]
    fn test_figma_color_to_hex() {
        let color = FigmaColor {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color.to_hex(), "#ff7f00");
    }

    #[test]
    fn test_figma_color_to_hex_black() {
        let color = FigmaColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color.to_hex(), "#000000");
    }

    #[tokio::test]
    async fn download_image_propagates_request_error() {
        let client = FigmaClient::new("token").expect("client");

        let result = client
            .download_image("http://127.0.0.1:1/nonexistent")
            .await;

        assert!(
            matches!(result, Err(FigmaError::Request(_))),
            "expected request error, got {:?}",
            result
        );
    }

    #[test]
    fn collect_figma_nodes_maps_typography_and_fills() {
        let child = FigmaNodeData {
            id: "2".to_string(),
            name: "Heading".to_string(),
            node_type: "TEXT".to_string(),
            children: vec![],
            absolute_bounding_box: Some(FigmaBoundingBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 30.0,
            }),
            characters: Some("Hello".to_string()),
            style: Some(FigmaTypeStyle {
                font_family: Some("Inter".to_string()),
                font_size: Some(16.0),
                font_weight: Some(600.0),
                line_height_px: Some(24.0),
            }),
            fills: vec![FigmaPaintData {
                paint_type: "SOLID".to_string(),
                color: Some(FigmaColor {
                    r: 0.062745,
                    g: 0.12549,
                    b: 0.188235,
                    a: 1.0,
                }),
                opacity: Some(0.8),
            }],
        };

        let root = FigmaNodeData {
            id: "1".to_string(),
            name: "Frame".to_string(),
            node_type: "FRAME".to_string(),
            children: vec![child],
            absolute_bounding_box: Some(FigmaBoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }),
            characters: None,
            style: None,
            fills: vec![],
        };

        let mut nodes = Vec::new();
        collect_figma_nodes(&root, &mut nodes);

        let text_node = nodes.iter().find(|n| n.id == "2").expect("text node");
        assert_eq!(text_node.text.as_deref(), Some("Hello"));
        let typo = text_node.typography.as_ref().expect("typography");
        assert_eq!(typo.font_family.as_deref(), Some("Inter"));
        assert_eq!(typo.font_size, Some(16.0));
        assert_eq!(typo.font_weight.as_deref(), Some("600"));
        assert_eq!(typo.line_height, Some(24.0));
        assert_eq!(text_node.fills.len(), 1);
        assert_eq!(text_node.fills[0].color.as_deref(), Some("#0f1f2f"));
        assert_eq!(text_node.fills[0].opacity, Some(0.8));

        let root_node = nodes.iter().find(|n| n.id == "1").expect("root node");
        assert_eq!(root_node.children, vec!["2"]);
        assert!((root_node.bounding_box.width - 1200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn finalize_figma_image_resizes_to_viewport() {
        let dir = TempDir::new().expect("tempdir");
        let out_path = dir.path().join("out.png");

        let img = RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255]));
        let img = DynamicImage::ImageRgba8(img);

        let (w, h, _) = finalize_figma_image(
            img,
            &out_path,
            Some(Viewport {
                width: 20,
                height: 20,
            }),
        )
        .expect("finalize");

        assert_eq!((w, h), (20, 20));
        let saved = image::open(&out_path).expect("open saved");
        assert_eq!(saved.dimensions(), (20, 20));
    }

    #[test]
    fn finalize_figma_image_keeps_original_size_when_no_viewport() {
        let dir = TempDir::new().expect("tempdir");
        let out_path = dir.path().join("out.png");

        let img = RgbaImage::from_pixel(12, 8, image::Rgba([0, 0, 255, 255]));
        let img = DynamicImage::ImageRgba8(img);

        let (w, h, transform) = finalize_figma_image(img, &out_path, None).expect("finalize");

        assert_eq!((w, h), (12, 8));
        let saved = image::open(&out_path).expect("open saved");
        assert_eq!(saved.dimensions(), (12, 8));
        assert_eq!(transform.scale, 1.0);
        assert_eq!(transform.offset_x, 0.0);
        assert_eq!(transform.offset_y, 0.0);
    }

    #[test]
    fn normalize_figma_snapshot_offsets_and_scales_to_viewport() {
        let root_bb = BoundingBox {
            x: 100.0,
            y: 200.0,
            width: 100.0,
            height: 50.0,
        };
        let mut snapshot = FigmaSnapshot {
            file_key: "FILE".into(),
            node_id: "root".into(),
            name: Some("Frame".into()),
            nodes: vec![
                FigmaNode {
                    id: "root".into(),
                    name: Some("Frame".into()),
                    node_type: "FRAME".into(),
                    bounding_box: root_bb,
                    text: None,
                    typography: None,
                    fills: vec![],
                    children: vec!["child".into()],
                },
                FigmaNode {
                    id: "child".into(),
                    name: Some("Text".into()),
                    node_type: "TEXT".into(),
                    bounding_box: BoundingBox {
                        x: 120.0,
                        y: 210.0,
                        width: 20.0,
                        height: 10.0,
                    },
                    text: Some("Hi".into()),
                    typography: Some(TypographyStyle {
                        font_family: Some("Inter".into()),
                        font_size: Some(16.0),
                        font_weight: Some("600".into()),
                        line_height: Some(24.0),
                    }),
                    fills: vec![],
                    children: vec![],
                },
            ],
        };

        let letterbox = compute_letterbox_transform(100, 50, 200, 200);
        snapshot = normalize_figma_snapshot(snapshot, Some(root_bb), (100, 50), &letterbox);

        let root = snapshot.nodes.iter().find(|n| n.id == "root").unwrap();
        assert_eq!(root.bounding_box.x, 0.0);
        assert_eq!(root.bounding_box.y, 50.0);
        assert_eq!(root.bounding_box.width, 200.0);
        assert_eq!(root.bounding_box.height, 100.0);

        let child = snapshot.nodes.iter().find(|n| n.id == "child").unwrap();
        assert!((child.bounding_box.x - 40.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.y - 70.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.width - 40.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.height - 20.0).abs() < f32::EPSILON);
    }
}
