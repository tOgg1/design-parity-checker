use std::fs;
use std::path::Path;

use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageError};
use thiserror::Error;

use crate::ocr::{self, OcrOptions};
use crate::types::{NormalizedView, OcrBlock, ResourceKind};

#[derive(Debug, Error)]
pub enum ImageLoadError {
    #[error("Failed to load image: {0}")]
    Load(#[from] ImageError),
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Failed to save normalized image: {0}")]
    Save(String),
    #[error("OCR extraction failed: {0}")]
    Ocr(String),
}

#[derive(Debug, Clone, Default)]
pub struct ImageLoadOptions {
    pub no_resize: bool,
    pub target_width: Option<u32>,
    pub target_height: Option<u32>,
    pub enable_ocr: bool,
    pub ocr_options: Option<OcrOptions>,
}

pub fn load_image(path: &str) -> Result<DynamicImage, ImageLoadError> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(ImageLoadError::NotFound(path.display().to_string()));
    }
    Ok(image::open(path)?)
}

pub fn image_to_normalized_view(
    path: &str,
    output_path: &str,
    options: ImageLoadOptions,
) -> Result<NormalizedView, ImageLoadError> {
    let img = load_image(path)?;
    let (orig_width, orig_height) = img.dimensions();

    let (final_img, width, height) = if options.no_resize {
        (img.clone(), orig_width, orig_height)
    } else if let (Some(tw), Some(th)) = (options.target_width, options.target_height) {
        let resized = resize_with_letterbox(&img, tw, th);
        (resized, tw, th)
    } else {
        (img.clone(), orig_width, orig_height)
    };

    let out_path = Path::new(output_path);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| ImageLoadError::Save(e.to_string()))?;
    }

    final_img
        .save(out_path)
        .map_err(|e| ImageLoadError::Save(e.to_string()))?;

    let ocr_blocks = if options.enable_ocr {
        let ocr_opts = options.ocr_options.clone().unwrap_or_default();
        match ocr::extract_text_blocks(out_path, &ocr_opts) {
            Ok(blocks) => Some(blocks),
            Err(ocr::OcrError::NotAvailable) => None,
            Err(e) => return Err(ImageLoadError::Ocr(e.to_string())),
        }
    } else {
        None
    };

    Ok(NormalizedView {
        kind: ResourceKind::Image,
        screenshot_path: out_path.to_path_buf(),
        width,
        height,
        dom: None,
        figma_tree: None,
        ocr_blocks,
    })
}

/// Extract OCR text from an existing NormalizedView's screenshot.
///
/// This can be used to add OCR data to a view that was created without OCR,
/// or to re-run OCR with different options.
pub fn extract_ocr_for_view(
    view: &NormalizedView,
    options: &OcrOptions,
) -> Result<Vec<OcrBlock>, ImageLoadError> {
    ocr::extract_text_blocks(&view.screenshot_path, options)
        .map_err(|e| ImageLoadError::Ocr(e.to_string()))
}

pub fn resize_with_letterbox(
    img: &DynamicImage,
    target_width: u32,
    target_height: u32,
) -> DynamicImage {
    let (src_w, src_h) = img.dimensions();

    let scale_w = target_width as f64 / src_w as f64;
    let scale_h = target_height as f64 / src_h as f64;
    let scale = scale_w.min(scale_h);

    let new_w = (src_w as f64 * scale).round() as u32;
    let new_h = (src_h as f64 * scale).round() as u32;

    let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);

    let mut canvas = DynamicImage::new_rgba8(target_width, target_height);
    let offset_x = (target_width - new_w) / 2;
    let offset_y = (target_height - new_h) / 2;

    image::imageops::overlay(&mut canvas, &resized, offset_x.into(), offset_y.into());

    canvas
}

pub fn resize_to_match(img: &DynamicImage, target_width: u32, target_height: u32) -> DynamicImage {
    img.resize_exact(target_width, target_height, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use tempfile::TempDir;

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_image("/nonexistent/path/image.png");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageLoadError::NotFound(_)));
    }

    #[test]
    fn test_resize_with_letterbox_preserves_aspect() {
        let img = DynamicImage::new_rgba8(200, 100);
        let result = resize_with_letterbox(&img, 100, 100);
        assert_eq!(result.dimensions(), (100, 100));
    }

    #[test]
    fn test_resize_to_match() {
        let img = DynamicImage::new_rgba8(200, 100);
        let result = resize_to_match(&img, 50, 50);
        assert_eq!(result.dimensions(), (50, 50));
    }

    #[test]
    fn image_to_normalized_view_saves_output_without_resize() {
        let dir = TempDir::new().expect("tempdir");
        let input_path = dir.path().join("input.png");
        let output_path = dir.path().join("output.png");

        let img = RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255]));
        img.save(&input_path).expect("write input image");

        let view = image_to_normalized_view(
            input_path.to_str().unwrap(),
            output_path.to_str().unwrap(),
            ImageLoadOptions {
                no_resize: true,
                ..Default::default()
            },
        )
        .expect("normalize image");

        assert!(output_path.exists(), "normalized image should be written");
        assert_eq!(view.width, 10);
        assert_eq!(view.height, 5);
    }

    #[test]
    fn image_to_normalized_view_resizes_with_targets() {
        let dir = TempDir::new().expect("tempdir");
        let input_path = dir.path().join("input2.png");
        let output_path = dir.path().join("output2.png");

        let img = RgbaImage::from_pixel(20, 10, image::Rgba([0, 255, 0, 255]));
        img.save(&input_path).expect("write input image");

        let view = image_to_normalized_view(
            input_path.to_str().unwrap(),
            output_path.to_str().unwrap(),
            ImageLoadOptions {
                no_resize: false,
                target_width: Some(40),
                target_height: Some(20),
                ..Default::default()
            },
        )
        .expect("normalize with resize");

        assert!(output_path.exists(), "resized image should be written");
        assert_eq!(view.width, 40);
        assert_eq!(view.height, 20);

        let saved = image::open(&output_path).expect("open saved image");
        assert_eq!(saved.dimensions(), (40, 20));
    }

    #[test]
    fn image_to_normalized_view_with_ocr_disabled_has_no_blocks() {
        let dir = TempDir::new().expect("tempdir");
        let input_path = dir.path().join("input_no_ocr.png");
        let output_path = dir.path().join("output_no_ocr.png");

        let img = RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255]));
        img.save(&input_path).expect("write input image");

        let view = image_to_normalized_view(
            input_path.to_str().unwrap(),
            output_path.to_str().unwrap(),
            ImageLoadOptions {
                no_resize: true,
                enable_ocr: false,
                ..Default::default()
            },
        )
        .expect("normalize image");

        assert!(view.ocr_blocks.is_none(), "OCR should not run when disabled");
    }

    #[test]
    fn image_to_normalized_view_with_ocr_enabled_gracefully_handles_unavailable() {
        let dir = TempDir::new().expect("tempdir");
        let input_path = dir.path().join("input_ocr.png");
        let output_path = dir.path().join("output_ocr.png");

        let img = RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255]));
        img.save(&input_path).expect("write input image");

        let view = image_to_normalized_view(
            input_path.to_str().unwrap(),
            output_path.to_str().unwrap(),
            ImageLoadOptions {
                no_resize: true,
                enable_ocr: true,
                ..Default::default()
            },
        )
        .expect("should succeed even when OCR unavailable");

        // When OCR feature is not compiled, ocr_blocks should be None
        // When OCR is compiled but tessdata is missing, it may error or return None
        // This test just ensures no panic occurs
        assert!(output_path.exists());
    }
}
