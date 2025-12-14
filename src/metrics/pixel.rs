use crate::error::DpcError;
use crate::image_loader::resize_to_match;
use crate::types::{
    DiffSeverity, NormalizedView, PixelDiffReason, PixelDiffRegion, PixelMetric,
};
use crate::Result;
use image::{DynamicImage, GenericImageView};

#[derive(Debug, Clone, Copy)]
pub struct PixelDiffThresholds {
    pub minor: f32,
    pub moderate: f32,
    pub major: f32,
}

impl Default for PixelDiffThresholds {
    fn default() -> Self {
        Self {
            minor: 0.05,
            moderate: 0.15,
            major: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PixelSimilarity {
    pub block_size: u32,
    pub thresholds: PixelDiffThresholds,
}

impl Default for PixelSimilarity {
    fn default() -> Self {
        Self {
            block_size: 32,
            thresholds: PixelDiffThresholds::default(),
        }
    }
}

impl PixelSimilarity {
    pub fn compute_metric(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<PixelMetric> {
        let (ref_img, mut impl_img) = load_images(reference, implementation)?;

        if ref_img.dimensions() != impl_img.dimensions() {
            let (w, h) = ref_img.dimensions();
            impl_img = resize_to_match(&impl_img, w, h);
        }

        let ref_luma = ref_img.to_luma8();
        let impl_luma = impl_img.to_luma8();

        let score = compute_ssim(&ref_luma, &impl_luma);
        let diff_map = compute_diff_map(&ref_luma, &impl_luma);
        let diff_regions = cluster_diff_regions(
            &diff_map,
            ref_luma.width(),
            ref_luma.height(),
            self.block_size,
            &self.thresholds,
        );

        Ok(PixelMetric {
            score,
            diff_regions,
        })
    }
}

fn load_images(
    reference: &NormalizedView,
    implementation: &NormalizedView,
) -> Result<(DynamicImage, DynamicImage)> {
    let ref_img = image::open(&reference.screenshot_path).map_err(DpcError::from)?;
    let impl_img = image::open(&implementation.screenshot_path).map_err(DpcError::from)?;
    Ok((ref_img, impl_img))
}

fn compute_ssim(ref_luma: &image::GrayImage, impl_luma: &image::GrayImage) -> f32 {
    let ref_buf = ref_luma.as_raw();
    let impl_buf = impl_luma.as_raw();

    let len = ref_buf.len().min(impl_buf.len());
    if len == 0 {
        return 1.0;
    }

    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_x2 = 0.0f64;
    let mut sum_y2 = 0.0f64;
    let mut sum_xy = 0.0f64;

    for i in 0..len {
        let x = ref_buf[i] as f64;
        let y = impl_buf[i] as f64;
        sum_x += x;
        sum_y += y;
        sum_x2 += x * x;
        sum_y2 += y * y;
        sum_xy += x * y;
    }

    let n = len as f64;
    let mu_x = sum_x / n;
    let mu_y = sum_y / n;
    let sigma_x = (sum_x2 / n) - mu_x * mu_x;
    let sigma_y = (sum_y2 / n) - mu_y * mu_y;
    let sigma_xy = (sum_xy / n) - mu_x * mu_y;

    let c1 = (0.01f64 * 255.0).powi(2);
    let c2 = (0.03f64 * 255.0).powi(2);

    let numerator = (2.0 * mu_x * mu_y + c1) * (2.0 * sigma_xy + c2);
    let denominator = (mu_x.powi(2) + mu_y.powi(2) + c1) * (sigma_x + sigma_y + c2);

    if denominator.abs() < f64::EPSILON {
        return 1.0;
    }

    let ssim = numerator / denominator;
    ssim.clamp(0.0, 1.0) as f32
}

fn compute_diff_map(ref_luma: &image::GrayImage, impl_luma: &image::GrayImage) -> Vec<f32> {
    let ref_buf = ref_luma.as_raw();
    let impl_buf = impl_luma.as_raw();
    let len = ref_buf.len().min(impl_buf.len());
    let mut diffs = Vec::with_capacity(len);

    for i in 0..len {
        let diff = (ref_buf[i] as f32 - impl_buf[i] as f32).abs() / 255.0;
        diffs.push(diff);
    }

    diffs
}

pub fn cluster_diff_regions(
    diff_map: &[f32],
    width: u32,
    height: u32,
    block_size: u32,
    thresholds: &PixelDiffThresholds,
) -> Vec<PixelDiffRegion> {
    if width == 0 || height == 0 || block_size == 0 || diff_map.is_empty() {
        return vec![];
    }

    let w = width as usize;
    let h = height as usize;
    let bs = block_size as usize;
    let mut regions = Vec::new();

    for y in (0..h).step_by(bs) {
        for x in (0..w).step_by(bs) {
            let block_w = bs.min(w - x);
            let block_h = bs.min(h - y);
            let mut sum = 0.0f32;
            for by in 0..block_h {
                let start = (y + by) * w + x;
                let end = start + block_w;
                sum += diff_map[start..end].iter().copied().sum::<f32>();
            }

            let avg = sum / (block_w * block_h) as f32;
            let severity = if avg >= thresholds.major {
                DiffSeverity::Major
            } else if avg >= thresholds.moderate {
                DiffSeverity::Moderate
            } else if avg >= thresholds.minor {
                DiffSeverity::Minor
            } else {
                continue;
            };

            regions.push(PixelDiffRegion {
                x: x as f32 / width as f32,
                y: y as f32 / height as f32,
                width: block_w as f32 / width as f32,
                height: block_h as f32 / height as f32,
                severity,
                reason: PixelDiffReason::PixelChange,
            });
        }
    }

    regions
}
