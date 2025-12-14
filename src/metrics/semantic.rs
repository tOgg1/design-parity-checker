//! Semantic analysis of diff regions using vision models.
//!
//! This module provides the ability to analyze clustered diff regions using
//! a multimodal LLM to determine what kind of visual differences exist.

use crate::error::DpcError;
use crate::types::DiffSeverity;
use base64::Engine;
use image::{DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::clustering::{
    cluster_regions_image_aware, ClusteredRegion, ImageAwareClusteringConfig,
};
use crate::types::PixelDiffRegion;

/// Type of semantic difference detected by vision analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDiffType {
    /// Text content differs (different words)
    TextContent,
    /// Text wrapping/line breaks differ
    TextReflow,
    /// Font family, size, or weight differs
    Typography,
    /// Element layout or positioning changed
    Layout,
    /// Colors are different
    Color,
    /// Element is missing from implementation
    MissingElement,
    /// Extra element in implementation
    ExtraElement,
    /// Spacing or padding differs
    Spacing,
    /// Image or icon differs
    ImageChange,
    /// Border or shadow differs
    Decoration,
    /// General visual difference
    Other,
}

impl std::fmt::Display for SemanticDiffType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextContent => write!(f, "text content change"),
            Self::TextReflow => write!(f, "text reflow/wrapping"),
            Self::Typography => write!(f, "typography difference"),
            Self::Layout => write!(f, "layout change"),
            Self::Color => write!(f, "color difference"),
            Self::MissingElement => write!(f, "missing element"),
            Self::ExtraElement => write!(f, "extra element"),
            Self::Spacing => write!(f, "spacing difference"),
            Self::ImageChange => write!(f, "image/icon change"),
            Self::Decoration => write!(f, "border/shadow change"),
            Self::Other => write!(f, "visual difference"),
        }
    }
}

/// A semantically analyzed diff region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDiff {
    /// Bounding box x (normalized 0.0-1.0)
    pub x: f32,
    /// Bounding box y (normalized 0.0-1.0)
    pub y: f32,
    /// Bounding box width (normalized 0.0-1.0)
    pub width: f32,
    /// Bounding box height (normalized 0.0-1.0)
    pub height: f32,
    /// Severity of the difference
    pub severity: DiffSeverity,
    /// Type of semantic difference
    pub diff_type: SemanticDiffType,
    /// Human-readable description of the difference
    pub description: String,
    /// Confidence score (0.0-1.0) from the vision model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Configuration for the semantic analyzer.
#[derive(Debug, Clone)]
pub struct SemanticAnalyzerConfig {
    /// OpenAI-compatible API endpoint
    pub api_endpoint: String,
    /// API key for authentication
    pub api_key: String,
    /// Model to use (e.g., "gpt-4o", "claude-3-5-sonnet")
    pub model: String,
    /// Maximum number of regions to analyze (to limit API costs)
    pub max_regions: usize,
    /// Minimum intensity threshold (0.0-1.0) for regions to analyze.
    /// Regions below this threshold are skipped as likely rendering noise.
    pub min_intensity: f32,
}

impl Default for SemanticAnalyzerConfig {
    fn default() -> Self {
        Self {
            api_endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            api_key: String::new(),
            model: "gpt-4o".to_string(),
            max_regions: 10,
            min_intensity: 0.08,
        }
    }
}

impl SemanticAnalyzerConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DPC_VISION_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok()?;

        let api_endpoint = std::env::var("DPC_VISION_API_ENDPOINT")
            .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());

        let model = std::env::var("DPC_VISION_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

        Some(Self {
            api_endpoint,
            api_key,
            model,
            max_regions: 10,
            min_intensity: 0.08,
        })
    }

    /// Create config from a SemanticConfig (from config file), with env vars as fallback.
    /// Returns None if no API key is available from either source.
    pub fn from_config(semantic_config: &crate::config::SemanticConfig) -> Option<Self> {
        let api_key = semantic_config
            .api_key
            .clone()
            .or_else(|| std::env::var("DPC_VISION_API_KEY").ok())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())?;

        let api_endpoint = semantic_config
            .api_endpoint
            .clone()
            .or_else(|| std::env::var("DPC_VISION_API_ENDPOINT").ok())
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

        let model = semantic_config
            .model
            .clone()
            .or_else(|| std::env::var("DPC_VISION_MODEL").ok())
            .unwrap_or_else(|| "gpt-4o".to_string());

        let max_regions = semantic_config.max_regions.unwrap_or(10);
        let min_intensity = semantic_config.min_intensity.unwrap_or(0.08);

        Some(Self {
            api_endpoint,
            api_key,
            model,
            max_regions,
            min_intensity,
        })
    }
}

/// Semantic analyzer for diff regions.
pub struct SemanticAnalyzer {
    config: SemanticAnalyzerConfig,
    client: reqwest::Client,
}

impl SemanticAnalyzer {
    /// Create a new semantic analyzer with the given configuration.
    pub fn new(config: SemanticAnalyzerConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Create from environment if API key is available.
    pub fn from_env() -> Option<Self> {
        SemanticAnalyzerConfig::from_env().map(Self::new)
    }

    /// Create from a SemanticConfig (from config file), with env vars as fallback.
    pub fn from_config(semantic_config: &crate::config::SemanticConfig) -> Option<Self> {
        SemanticAnalyzerConfig::from_config(semantic_config).map(Self::new)
    }

    /// Analyze raw pixel diff regions using image-aware clustering + vision model.
    ///
    /// This method performs smart clustering that groups visually similar regions together,
    /// separating different UI components (e.g., product grid vs pricing panel).
    ///
    /// `context` is an optional description of what the images show.
    pub async fn analyze_diff_regions(
        &self,
        ref_image_path: &Path,
        impl_image_path: &Path,
        diff_regions: &[PixelDiffRegion],
        context: Option<&str>,
    ) -> Result<Vec<SemanticDiff>, DpcError> {
        if diff_regions.is_empty() {
            return Ok(vec![]);
        }

        let ref_img = image::open(ref_image_path).map_err(DpcError::from)?;

        // Use image-aware clustering to group visually similar regions
        let clustering_config = ImageAwareClusteringConfig::default();
        let clustered = cluster_regions_image_aware(diff_regions, &ref_img, &clustering_config);

        self.analyze_regions(ref_image_path, impl_image_path, &clustered, context)
            .await
    }

    /// Analyze pre-clustered diff regions using the vision model.
    ///
    /// `context` is an optional description of what the images show (e.g., "Home alarm signup page with partner logos").
    pub async fn analyze_regions(
        &self,
        ref_image_path: &Path,
        impl_image_path: &Path,
        regions: &[ClusteredRegion],
        context: Option<&str>,
    ) -> Result<Vec<SemanticDiff>, DpcError> {
        if regions.is_empty() {
            return Ok(vec![]);
        }

        let ref_img = image::open(ref_image_path).map_err(DpcError::from)?;
        let impl_img = image::open(impl_image_path).map_err(DpcError::from)?;

        // Create thumbnails of full images for context (resize to max 800px on longest side)
        let ref_thumb = create_thumbnail(&ref_img, 800);
        let impl_thumb = create_thumbnail(&impl_img, 800);

        // Encode full thumbnails once (reused for all regions)
        let ref_full_b64 = encode_image_base64(&ref_thumb)?;
        let impl_full_b64 = encode_image_base64(&impl_thumb)?;

        let regions_to_analyze: Vec<_> = regions
            .iter()
            .filter(|r| r.intensity >= self.config.min_intensity)
            .take(self.config.max_regions)
            .collect();
        let mut results = Vec::new();

        for region in regions_to_analyze {
            match self
                .analyze_single_region(&ref_img, &impl_img, &ref_full_b64, &impl_full_b64, region, context)
                .await
            {
                Ok(diff) => results.push(diff),
                Err(e) => {
                    eprintln!("Warning: Failed to analyze region: {}", e);
                    results.push(SemanticDiff {
                        x: region.x,
                        y: region.y,
                        width: region.width,
                        height: region.height,
                        severity: region.severity,
                        diff_type: SemanticDiffType::Other,
                        description: "Visual difference detected".to_string(),
                        confidence: None,
                    });
                }
            }
        }

        Ok(results)
    }

    async fn analyze_single_region(
        &self,
        ref_img: &DynamicImage,
        impl_img: &DynamicImage,
        ref_full_b64: &str,
        impl_full_b64: &str,
        region: &ClusteredRegion,
        context: Option<&str>,
    ) -> Result<SemanticDiff, DpcError> {
        let (ref_w, ref_h) = ref_img.dimensions();

        // Expand region slightly for context (10% padding)
        let padding = 0.05;
        let x = (region.x - padding).max(0.0);
        let y = (region.y - padding).max(0.0);
        let w = (region.width + 2.0 * padding).min(1.0 - x);
        let h = (region.height + 2.0 * padding).min(1.0 - y);

        // Convert to pixel coordinates
        let px = (x * ref_w as f32) as u32;
        let py = (y * ref_h as f32) as u32;
        let pw = (w * ref_w as f32) as u32;
        let ph = (h * ref_h as f32) as u32;

        // Crop regions
        let ref_crop = ref_img.crop_imm(px, py, pw.max(1), ph.max(1));
        let impl_crop = impl_img.crop_imm(px, py, pw.max(1), ph.max(1));

        // Encode cropped regions as base64
        let ref_crop_b64 = encode_image_base64(&ref_crop)?;
        let impl_crop_b64 = encode_image_base64(&impl_crop)?;

        // Call vision API with both full images and cropped regions
        let analysis = self
            .call_vision_api(ref_full_b64, impl_full_b64, &ref_crop_b64, &impl_crop_b64, region, context)
            .await?;

        Ok(SemanticDiff {
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
            severity: region.severity,
            diff_type: analysis.diff_type,
            description: analysis.description,
            confidence: analysis.confidence,
        })
    }

    async fn call_vision_api(
        &self,
        ref_full_b64: &str,
        impl_full_b64: &str,
        ref_crop_b64: &str,
        impl_crop_b64: &str,
        region: &ClusteredRegion,
        context: Option<&str>,
    ) -> Result<VisionAnalysis, DpcError> {
        let intensity_pct = (region.intensity * 100.0).round();
        let intensity_label = if region.intensity < 0.10 {
            "very low (likely subtle: anti-aliasing, subpixel rendering, border-radius)"
        } else if region.intensity < 0.20 {
            "low (minor difference)"
        } else if region.intensity < 0.40 {
            "moderate"
        } else {
            "high (significant visual difference)"
        };

        let region_desc = format!(
            "REGION LOCATION: position ({:.0}%, {:.0}%) from top-left, size ({:.0}% x {:.0}%) of viewport.\n\
             PIXEL DIFF INTENSITY: {:.0}% — {} intensity.",
            region.x * 100.0,
            region.y * 100.0,
            region.width * 100.0,
            region.height * 100.0,
            intensity_pct,
            intensity_label
        );

        let context_section = context
            .map(|c| format!("\n\nCONTEXT: {}\n", c))
            .unwrap_or_default();

        let prompt = format!(r#"You are a design QA expert comparing a REFERENCE design against an IMPLEMENTATION.{context_section}

I'm providing:
1. Full REFERENCE image (the design specification)
2. Full IMPLEMENTATION image (what was actually built)
3. Cropped REFERENCE region (zoomed in on the difference area)
4. Cropped IMPLEMENTATION region (zoomed in on the same area)

{region_desc}

Your task: Identify WHAT specifically changed and describe it in terms a developer can act on.

IMPORTANT GUIDELINES:
- For LOW intensity diffs (<15%): These are often subtle rendering differences like border-radius, anti-aliasing, font smoothing, or subpixel rendering. Be specific if you can identify it, otherwise describe as "minor rendering difference" with high confidence.
- Only describe differences you can ACTUALLY SEE in the cropped images. If both crops look identical to you, say "No visible difference detected" with diff_type "other".
- Focus on what UI element this is and what specifically differs.
- Be concrete and actionable.

Respond in JSON:
{{
  "diff_type": "text_content|text_reflow|typography|layout|color|missing_element|extra_element|spacing|image_change|decoration|other",
  "description": "Specific, actionable description of what changed. If unsure, describe what you see.",
  "confidence": 0.0 to 1.0
}}

Examples of good descriptions:
- "Border-radius on card corners slightly different (design has 8px, implementation appears 4px)"
- "Partner logo section: logos arranged in 2 rows instead of 1 horizontal row"
- "Minor anti-aliasing difference on text edges - no actionable change needed"
- "No visible difference detected - likely subpixel rendering variation""#);

        let payload = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": prompt },
                        {
                            "type": "image_url",
                            "image_url": { "url": format!("data:image/png;base64,{}", ref_full_b64), "detail": "low" }
                        },
                        {
                            "type": "image_url",
                            "image_url": { "url": format!("data:image/png;base64,{}", impl_full_b64), "detail": "low" }
                        },
                        {
                            "type": "image_url",
                            "image_url": { "url": format!("data:image/png;base64,{}", ref_crop_b64), "detail": "high" }
                        },
                        {
                            "type": "image_url",
                            "image_url": { "url": format!("data:image/png;base64,{}", impl_crop_b64), "detail": "high" }
                        }
                    ]
                }
            ],
            "max_tokens": 250,
            "response_format": { "type": "json_object" }
        });

        let response = self
            .client
            .post(&self.config.api_endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| DpcError::metric(format!("Vision API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(DpcError::metric(format!(
                "Vision API returned {}: {}",
                status, body
            )));
        }

        let resp: VisionApiResponse = response
            .json()
            .await
            .map_err(|e| DpcError::metric(format!("Failed to parse vision API response: {}", e)))?;

        let content = resp
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
            .ok_or_else(|| DpcError::metric("Empty vision API response"))?;

        let analysis: VisionAnalysis = serde_json::from_str(content)
            .map_err(|e| DpcError::metric(format!("Failed to parse analysis JSON: {}", e)))?;

        Ok(analysis)
    }
}

fn encode_image_base64(img: &DynamicImage) -> Result<String, DpcError> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, image::ImageOutputFormat::Png)
        .map_err(|e| DpcError::metric(format!("Failed to encode image: {}", e)))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&buf))
}

fn create_thumbnail(img: &DynamicImage, max_size: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w <= max_size && h <= max_size {
        return img.clone();
    }

    let scale = if w > h {
        max_size as f32 / w as f32
    } else {
        max_size as f32 / h as f32
    };

    let new_w = (w as f32 * scale) as u32;
    let new_h = (h as f32 * scale) as u32;

    img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
}

#[derive(Debug, Deserialize)]
struct VisionApiResponse {
    choices: Vec<VisionChoice>,
}

#[derive(Debug, Deserialize)]
struct VisionChoice {
    message: VisionMessage,
}

#[derive(Debug, Deserialize)]
struct VisionMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisionAnalysis {
    diff_type: SemanticDiffType,
    description: String,
    confidence: Option<f32>,
}

/// Batch analyze multiple regions in a single API call (more efficient).
pub async fn analyze_regions_batch(
    config: &SemanticAnalyzerConfig,
    ref_image_path: &Path,
    impl_image_path: &Path,
    regions: &[ClusteredRegion],
    context: Option<&str>,
) -> Result<Vec<SemanticDiff>, DpcError> {
    let analyzer = SemanticAnalyzer::new(config.clone());
    analyzer
        .analyze_regions(ref_image_path, impl_image_path, regions, context)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_diff_type_display() {
        assert_eq!(
            format!("{}", SemanticDiffType::TextReflow),
            "text reflow/wrapping"
        );
        assert_eq!(format!("{}", SemanticDiffType::Layout), "layout change");
    }

    #[test]
    fn config_defaults_are_sensible() {
        let config = SemanticAnalyzerConfig::default();
        assert!(config.api_endpoint.contains("openai"));
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.max_regions, 10);
    }
}
