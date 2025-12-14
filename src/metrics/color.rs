use crate::error::DpcError;
use crate::types::{ColorDiff, ColorDiffKind, ColorMetric, NormalizedView};
use crate::Result;
use image::{DynamicImage, GenericImageView};
use palette::{convert::FromColorUnclamped, Lab, Srgb};

#[derive(Debug, Clone, Copy)]
pub struct ColorPaletteMetric {
    pub clusters: usize,
    pub sample_stride: u32,
}

impl Default for ColorPaletteMetric {
    fn default() -> Self {
        Self {
            clusters: 5,
            sample_stride: 4,
        }
    }
}

impl ColorPaletteMetric {
    pub fn compute_metric(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<ColorMetric> {
        let ref_img = image::open(&reference.screenshot_path).map_err(DpcError::from)?;
        let impl_img = image::open(&implementation.screenshot_path).map_err(DpcError::from)?;

        let ref_palette = dominant_palette(&ref_img, self.clusters, self.sample_stride);
        let impl_palette = dominant_palette(&impl_img, self.clusters, self.sample_stride);

        let mut diffs = palette_diffs(&ref_palette, &impl_palette, 3);
        let mut score = palette_similarity(&ref_palette, &impl_palette);
        let needs_fallback = diffs.is_empty()
            || diffs
                .iter()
                .all(|d| d.ref_color == d.impl_color && d.delta_e.unwrap_or(0.0) <= 1.0);

        if needs_fallback {
            let avg_ref = average_rgb(&ref_img);
            let avg_impl = average_rgb(&impl_img);
            let delta = rgb_distance(&avg_ref, &avg_impl);
            diffs.push(ColorDiff {
                kind: ColorDiffKind::PrimaryColorShift,
                ref_color: format!("#{:02X}{:02X}{:02X}", avg_ref[0], avg_ref[1], avg_ref[2]),
                impl_color: format!("#{:02X}{:02X}{:02X}", avg_impl[0], avg_impl[1], avg_impl[2]),
                delta_e: Some(delta),
            });
        }

        let has_meaningful_diff = diffs
            .iter()
            .any(|d| d.ref_color != d.impl_color || d.delta_e.unwrap_or(0.0) > 1.0);
        if has_meaningful_diff {
            score = score.min(0.8);
        }

        Ok(ColorMetric { score, diffs })
    }
}

fn dominant_palette(img: &DynamicImage, clusters: usize, stride: u32) -> Vec<(Lab, f32)> {
    let samples = sample_pixels(img, stride);
    if samples.is_empty() {
        return Vec::new();
    }

    let k = clusters.max(1).min(samples.len());
    kmeans(&samples, k, 8)
}

fn sample_pixels(img: &DynamicImage, stride: u32) -> Vec<(Lab, f32)> {
    let (w, h) = img.dimensions();
    let mut samples = Vec::new();
    let step = stride.max(1);

    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let pixel = img.get_pixel(x, y).0;
            let srgb = Srgb::new(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            );
            let lab: Lab = Lab::from_color_unclamped(srgb);
            samples.push((lab, 1.0));
        }
    }

    samples
}

fn kmeans(samples: &[(Lab, f32)], k: usize, iterations: usize) -> Vec<(Lab, f32)> {
    let mut centers: Vec<Lab> = Vec::with_capacity(k);
    let mut weights = Vec::with_capacity(k);

    let step = (samples.len() / k).max(1);
    for i in 0..k {
        centers.push(samples[i * step % samples.len()].0);
    }

    for _ in 0..iterations {
        let mut accum = vec![(0.0f32, 0.0f32, 0.0f32, 0.0f32); k];
        weights.clear();
        weights.resize(k, 0.0f32);

        for (lab, w) in samples {
            let (idx, _) = centers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, lab_distance2(*lab, *c)))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();

            accum[idx].0 += lab.l * w;
            accum[idx].1 += lab.a * w;
            accum[idx].2 += lab.b * w;
            accum[idx].3 += w;
            weights[idx] += *w;
        }

        for i in 0..k {
            if accum[i].3 > 0.0 {
                centers[i] = Lab::new(
                    accum[i].0 / accum[i].3,
                    accum[i].1 / accum[i].3,
                    accum[i].2 / accum[i].3,
                );
            }
        }
    }

    let total_weight: f32 = weights.iter().copied().sum::<f32>().max(f32::EPSILON);
    centers
        .into_iter()
        .zip(weights)
        .map(|(c, w)| (c, w / total_weight))
        .collect()
}

fn palette_similarity(ref_palette: &[(Lab, f32)], impl_palette: &[(Lab, f32)]) -> f32 {
    if ref_palette.is_empty() || impl_palette.is_empty() {
        return 0.0;
    }

    ref_palette
        .iter()
        .map(|(lab_ref, weight)| {
            let delta = impl_palette
                .iter()
                .map(|(lab_impl, _)| lab_distance2(*lab_ref, *lab_impl).sqrt())
                .fold(f32::INFINITY, f32::min);

            let match_score = 1.0 - (delta / 25.0).min(1.0);
            weight * match_score
        })
        .sum::<f32>()
}

fn palette_diffs(
    ref_palette: &[(Lab, f32)],
    impl_palette: &[(Lab, f32)],
    top_n: usize,
) -> Vec<ColorDiff> {
    if ref_palette.is_empty() || impl_palette.is_empty() {
        return Vec::new();
    }

    let mut sorted = ref_palette.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(top_n);

    let mut diffs = Vec::new();

    for (idx, (lab_ref, _w)) in sorted.iter().enumerate() {
        if let Some((lab_impl, delta)) = impl_palette
            .iter()
            .map(|(l, _)| {
                let d = lab_distance2(*lab_ref, *l).sqrt();
                (l, d)
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        {
            let kind = match idx {
                0 => ColorDiffKind::PrimaryColorShift,
                1 => ColorDiffKind::AccentColorShift,
                _ => ColorDiffKind::BackgroundColorShift,
            };
            diffs.push(ColorDiff {
                kind,
                ref_color: lab_to_hex(*lab_ref),
                impl_color: lab_to_hex(*lab_impl),
                delta_e: Some(delta),
            });
        }
    }

    diffs
}

fn average_rgb(img: &DynamicImage) -> [u8; 3] {
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for (_x, _y, pixel) in img.pixels() {
        let c = pixel.0;
        sum[0] += c[0] as u64;
        sum[1] += c[1] as u64;
        sum[2] += c[2] as u64;
        count += 1;
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ]
}

fn rgb_distance(a: &[u8; 3], b: &[u8; 3]) -> f32 {
    let dr = a[0] as f32 - b[0] as f32;
    let dg = a[1] as f32 - b[1] as f32;
    let db = a[2] as f32 - b[2] as f32;
    ((dr * dr + dg * dg + db * db).sqrt()).max(0.0)
}

fn lab_to_hex(lab: Lab) -> String {
    let srgb: Srgb = Srgb::from_color_unclamped(lab);
    let clamp = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        clamp(srgb.red),
        clamp(srgb.green),
        clamp(srgb.blue)
    )
}

fn lab_distance2(a: Lab, b: Lab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    dl * dl + da * da + db * db
}
