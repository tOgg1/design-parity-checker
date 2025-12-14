//! Clustering algorithms for merging adjacent diff regions.
//!
//! This module provides algorithms to cluster nearby pixel diff regions into
//! larger bounding boxes, making the output more actionable and reducing noise.

use crate::types::{DiffSeverity, PixelDiffReason, PixelDiffRegion};
use image::{DynamicImage, GenericImageView};

/// Configuration for region clustering.
#[derive(Debug, Clone, Copy)]
pub struct ClusteringConfig {
    /// Maximum gap (as fraction of image dimension) between regions to merge.
    /// Regions within this distance will be clustered together.
    pub gap_threshold: f32,
    /// Minimum number of regions to form a cluster.
    pub min_cluster_size: usize,
}

impl Default for ClusteringConfig {
    fn default() -> Self {
        Self {
            gap_threshold: 0.05, // 5% of image dimension
            min_cluster_size: 1,
        }
    }
}

/// Configuration for image-aware clustering that considers visual similarity.
#[derive(Debug, Clone, Copy)]
pub struct ImageAwareClusteringConfig {
    /// Maximum gap (as fraction of image dimension) between regions to merge.
    pub gap_threshold: f32,
    /// Minimum color similarity (0.0-1.0) required to merge regions.
    /// 1.0 = identical colors, 0.0 = completely different.
    pub color_similarity_threshold: f32,
    /// Number of pixels to sample per region for color analysis.
    pub sample_count: usize,
}

impl Default for ImageAwareClusteringConfig {
    fn default() -> Self {
        Self {
            gap_threshold: 0.05,
            color_similarity_threshold: 0.85, // Require 85% color similarity
            sample_count: 16,
        }
    }
}

/// A clustered region combining multiple adjacent diff regions.
#[derive(Debug, Clone)]
pub struct ClusteredRegion {
    /// Bounding box x (normalized 0.0-1.0)
    pub x: f32,
    /// Bounding box y (normalized 0.0-1.0)
    pub y: f32,
    /// Bounding box width (normalized 0.0-1.0)
    pub width: f32,
    /// Bounding box height (normalized 0.0-1.0)
    pub height: f32,
    /// Highest severity among constituent regions
    pub severity: DiffSeverity,
    /// Number of original regions in this cluster
    pub region_count: usize,
    /// Average diff intensity (0.0-1.0) across the cluster
    pub intensity: f32,
}

impl ClusteredRegion {
    /// Convert back to a PixelDiffRegion for output compatibility.
    pub fn to_pixel_diff_region(&self) -> PixelDiffRegion {
        PixelDiffRegion {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            severity: self.severity,
            reason: PixelDiffReason::PixelChange,
            intensity: Some(self.intensity),
        }
    }
}

/// Cluster adjacent diff regions using a union-find approach with spatial proximity.
///
/// This algorithm:
/// 1. Builds a graph where regions are connected if they're within `gap_threshold` distance
/// 2. Finds connected components using union-find
/// 3. Merges each component into a single bounding box with aggregated severity
pub fn cluster_regions(
    regions: &[PixelDiffRegion],
    config: &ClusteringConfig,
) -> Vec<ClusteredRegion> {
    if regions.is_empty() {
        return vec![];
    }

    let n = regions.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    // Union-find helpers
    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], rank: &mut [usize], i: usize, j: usize) {
        let pi = find(parent, i);
        let pj = find(parent, j);
        if pi == pj {
            return;
        }
        if rank[pi] < rank[pj] {
            parent[pi] = pj;
        } else if rank[pi] > rank[pj] {
            parent[pj] = pi;
        } else {
            parent[pj] = pi;
            rank[pi] += 1;
        }
    }

    // Check if two regions are close enough to cluster
    fn regions_adjacent(a: &PixelDiffRegion, b: &PixelDiffRegion, gap: f32) -> bool {
        let a_right = a.x + a.width;
        let a_bottom = a.y + a.height;
        let b_right = b.x + b.width;
        let b_bottom = b.y + b.height;

        // Check horizontal gap
        let h_gap = if a_right < b.x {
            b.x - a_right
        } else if b_right < a.x {
            a.x - b_right
        } else {
            0.0 // Overlapping horizontally
        };

        // Check vertical gap
        let v_gap = if a_bottom < b.y {
            b.y - a_bottom
        } else if b_bottom < a.y {
            a.y - b_bottom
        } else {
            0.0 // Overlapping vertically
        };

        // Regions are adjacent if the gap in both dimensions is within threshold
        h_gap <= gap && v_gap <= gap
    }

    // Build adjacency and union
    for i in 0..n {
        for j in (i + 1)..n {
            if regions_adjacent(&regions[i], &regions[j], config.gap_threshold) {
                union(&mut parent, &mut rank, i, j);
            }
        }
    }

    // Group by root
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Merge each group into a clustered region
    let mut clustered = Vec::new();
    for indices in groups.values() {
        if indices.len() < config.min_cluster_size {
            // Keep as individual regions if below threshold
            for &i in indices {
                let intensity = regions[i]
                    .intensity
                    .unwrap_or_else(|| severity_to_intensity(regions[i].severity));
                clustered.push(ClusteredRegion {
                    x: regions[i].x,
                    y: regions[i].y,
                    width: regions[i].width,
                    height: regions[i].height,
                    severity: regions[i].severity,
                    region_count: 1,
                    intensity,
                });
            }
            continue;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_severity = DiffSeverity::Minor;
        let mut intensity_sum = 0.0f32;

        for &i in indices {
            let r = &regions[i];
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.width);
            max_y = max_y.max(r.y + r.height);
            max_severity = max_severity.max(r.severity);
            intensity_sum += r.intensity.unwrap_or_else(|| severity_to_intensity(r.severity));
        }

        clustered.push(ClusteredRegion {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
            severity: max_severity,
            region_count: indices.len(),
            intensity: intensity_sum / indices.len() as f32,
        });
    }

    // Sort by severity (descending) then by area (descending)
    clustered.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| {
                let area_a = a.width * a.height;
                let area_b = b.width * b.height;
                area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    clustered
}

fn severity_to_intensity(severity: DiffSeverity) -> f32 {
    match severity {
        DiffSeverity::Minor => 0.33,
        DiffSeverity::Moderate => 0.66,
        DiffSeverity::Major => 1.0,
    }
}

/// Cluster diff regions using both spatial proximity and visual similarity.
///
/// This variant only merges regions that are:
/// 1. Spatially close (within gap_threshold)
/// 2. Visually similar (similar background colors)
///
/// This helps separate different UI components (e.g., product grid vs pricing panel)
/// even if they have adjacent diff regions.
pub fn cluster_regions_image_aware(
    regions: &[PixelDiffRegion],
    ref_image: &DynamicImage,
    config: &ImageAwareClusteringConfig,
) -> Vec<ClusteredRegion> {
    if regions.is_empty() {
        return vec![];
    }

    let (img_w, img_h) = ref_image.dimensions();
    let n = regions.len();

    // Pre-compute color signatures for each region
    let signatures: Vec<ColorSignature> = regions
        .iter()
        .map(|r| extract_color_signature(ref_image, r, img_w, img_h, config.sample_count))
        .collect();

    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], rank: &mut [usize], x: usize, y: usize) {
        let px = find(parent, x);
        let py = find(parent, y);
        if px == py {
            return;
        }
        if rank[px] < rank[py] {
            parent[px] = py;
        } else if rank[px] > rank[py] {
            parent[py] = px;
        } else {
            parent[py] = px;
            rank[px] += 1;
        }
    }

    fn regions_adjacent(a: &PixelDiffRegion, b: &PixelDiffRegion, gap: f32) -> bool {
        let a_right = a.x + a.width;
        let a_bottom = a.y + a.height;
        let b_right = b.x + b.width;
        let b_bottom = b.y + b.height;

        let h_gap = if a_right < b.x {
            b.x - a_right
        } else if b_right < a.x {
            a.x - b_right
        } else {
            0.0
        };

        let v_gap = if a_bottom < b.y {
            b.y - a_bottom
        } else if b_bottom < a.y {
            a.y - b_bottom
        } else {
            0.0
        };

        h_gap <= gap && v_gap <= gap
    }

    // Build adjacency and union - only if spatially close AND visually similar
    for i in 0..n {
        for j in (i + 1)..n {
            if regions_adjacent(&regions[i], &regions[j], config.gap_threshold) {
                let similarity = signatures[i].similarity(&signatures[j]);
                if similarity >= config.color_similarity_threshold {
                    union(&mut parent, &mut rank, i, j);
                }
            }
        }
    }

    // Group by root
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Merge each group into a clustered region
    let mut clustered = Vec::new();
    for indices in groups.values() {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_severity = DiffSeverity::Minor;
        let mut intensity_sum = 0.0f32;

        for &i in indices {
            let r = &regions[i];
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.width);
            max_y = max_y.max(r.y + r.height);
            max_severity = max_severity.max(r.severity);
            intensity_sum += r.intensity.unwrap_or_else(|| severity_to_intensity(r.severity));
        }

        clustered.push(ClusteredRegion {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
            severity: max_severity,
            region_count: indices.len(),
            intensity: intensity_sum / indices.len() as f32,
        });
    }

    // Sort by severity (descending) then by area (descending)
    clustered.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| {
                let area_a = a.width * a.height;
                let area_b = b.width * b.height;
                area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    clustered
}

/// A color signature for a region, used for visual similarity comparison.
#[derive(Debug, Clone)]
struct ColorSignature {
    /// Average RGB values (0.0-1.0 each)
    avg_r: f32,
    avg_g: f32,
    avg_b: f32,
    /// Brightness variance (indicates texture complexity)
    brightness_variance: f32,
}

impl ColorSignature {
    /// Compute similarity between two signatures (0.0-1.0).
    fn similarity(&self, other: &Self) -> f32 {
        // Color distance (Euclidean in RGB space, normalized)
        let dr = self.avg_r - other.avg_r;
        let dg = self.avg_g - other.avg_g;
        let db = self.avg_b - other.avg_b;
        let color_dist = (dr * dr + dg * dg + db * db).sqrt();
        // Max distance in RGB cube is sqrt(3) ≈ 1.73
        let color_similarity = 1.0 - (color_dist / 1.73);

        // Variance similarity (both high-detail or both low-detail regions)
        let variance_diff = (self.brightness_variance - other.brightness_variance).abs();
        let variance_similarity = 1.0 - variance_diff.min(1.0);

        // Weight color more heavily than variance
        0.8 * color_similarity + 0.2 * variance_similarity
    }
}

/// Extract a color signature from a region of the image.
fn extract_color_signature(
    image: &DynamicImage,
    region: &PixelDiffRegion,
    img_w: u32,
    img_h: u32,
    sample_count: usize,
) -> ColorSignature {
    let px = (region.x * img_w as f32) as u32;
    let py = (region.y * img_h as f32) as u32;
    let pw = ((region.width * img_w as f32) as u32).max(1);
    let ph = ((region.height * img_h as f32) as u32).max(1);

    // Sample points in a grid pattern
    let samples_per_dim = (sample_count as f32).sqrt().ceil() as u32;
    let step_x = pw / samples_per_dim.max(1);
    let step_y = ph / samples_per_dim.max(1);

    let mut sum_r = 0.0f32;
    let mut sum_g = 0.0f32;
    let mut sum_b = 0.0f32;
    let mut brightnesses = Vec::new();
    let mut count = 0;

    for sy in 0..samples_per_dim {
        for sx in 0..samples_per_dim {
            let x = (px + sx * step_x.max(1)).min(img_w - 1);
            let y = (py + sy * step_y.max(1)).min(img_h - 1);

            let pixel = image.get_pixel(x, y);
            let r = pixel[0] as f32 / 255.0;
            let g = pixel[1] as f32 / 255.0;
            let b = pixel[2] as f32 / 255.0;

            sum_r += r;
            sum_g += g;
            sum_b += b;
            brightnesses.push(0.299 * r + 0.587 * g + 0.114 * b);
            count += 1;
        }
    }

    let count_f = count.max(1) as f32;
    let avg_r = sum_r / count_f;
    let avg_g = sum_g / count_f;
    let avg_b = sum_b / count_f;

    // Compute brightness variance
    let avg_brightness: f32 = brightnesses.iter().sum::<f32>() / count_f;
    let brightness_variance = if count > 1 {
        brightnesses
            .iter()
            .map(|b| (b - avg_brightness).powi(2))
            .sum::<f32>()
            / count_f
    } else {
        0.0
    };

    ColorSignature {
        avg_r,
        avg_g,
        avg_b,
        brightness_variance,
    }
}

/// Convert clustered regions back to PixelDiffRegion format for output compatibility.
pub fn clustered_to_pixel_regions(clustered: &[ClusteredRegion]) -> Vec<PixelDiffRegion> {
    clustered.iter().map(|c| c.to_pixel_diff_region()).collect()
}

// Make DiffSeverity orderable for max comparison
impl PartialOrd for DiffSeverity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DiffSeverity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let self_val = match self {
            DiffSeverity::Minor => 0,
            DiffSeverity::Moderate => 1,
            DiffSeverity::Major => 2,
        };
        let other_val = match other {
            DiffSeverity::Minor => 0,
            DiffSeverity::Moderate => 1,
            DiffSeverity::Major => 2,
        };
        self_val.cmp(&other_val)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_region(x: f32, y: f32, w: f32, h: f32, severity: DiffSeverity) -> PixelDiffRegion {
        PixelDiffRegion {
            x,
            y,
            width: w,
            height: h,
            severity,
            reason: PixelDiffReason::PixelChange,
            intensity: None,
        }
    }

    #[test]
    fn clusters_adjacent_regions() {
        let regions = vec![
            make_region(0.0, 0.0, 0.1, 0.1, DiffSeverity::Minor),
            make_region(0.1, 0.0, 0.1, 0.1, DiffSeverity::Moderate),
            make_region(0.2, 0.0, 0.1, 0.1, DiffSeverity::Minor),
            make_region(0.8, 0.8, 0.1, 0.1, DiffSeverity::Major), // Far away
        ];

        let config = ClusteringConfig {
            gap_threshold: 0.05,
            min_cluster_size: 1,
        };

        let clustered = cluster_regions(&regions, &config);
        assert_eq!(clustered.len(), 2, "should have 2 clusters");

        // First cluster should contain the 3 adjacent regions
        let large_cluster = clustered.iter().find(|c| c.region_count == 3).unwrap();
        assert_eq!(large_cluster.severity, DiffSeverity::Moderate);
        assert!((large_cluster.width - 0.3).abs() < 0.01);

        // Second cluster is the isolated region
        let isolated = clustered.iter().find(|c| c.region_count == 1).unwrap();
        assert_eq!(isolated.severity, DiffSeverity::Major);
    }

    #[test]
    fn empty_regions_returns_empty() {
        let clustered = cluster_regions(&[], &ClusteringConfig::default());
        assert!(clustered.is_empty());
    }

    #[test]
    fn non_adjacent_regions_stay_separate() {
        let regions = vec![
            make_region(0.0, 0.0, 0.1, 0.1, DiffSeverity::Minor),
            make_region(0.5, 0.5, 0.1, 0.1, DiffSeverity::Minor),
        ];

        let config = ClusteringConfig {
            gap_threshold: 0.05,
            min_cluster_size: 1,
        };

        let clustered = cluster_regions(&regions, &config);
        assert_eq!(clustered.len(), 2, "non-adjacent regions stay separate");
    }

    #[test]
    fn image_aware_separates_by_color() {
        // Create a test image with two distinct regions: white left, beige right
        let mut img = image::RgbImage::new(100, 100);
        for x in 0..100 {
            for y in 0..100 {
                let color = if x < 60 {
                    image::Rgb([255, 255, 255]) // White
                } else {
                    image::Rgb([241, 239, 234]) // Beige (#F1EFEA)
                };
                img.put_pixel(x, y, color);
            }
        }
        let dynamic_img = DynamicImage::ImageRgb8(img);

        // Two adjacent regions, but on different colored backgrounds
        let regions = vec![
            PixelDiffRegion {
                x: 0.3,
                y: 0.3,
                width: 0.2,
                height: 0.2,
                severity: DiffSeverity::Major,
                reason: PixelDiffReason::PixelChange,
                intensity: Some(0.4),
            },
            PixelDiffRegion {
                x: 0.65,
                y: 0.3,
                width: 0.2,
                height: 0.2,
                severity: DiffSeverity::Major,
                reason: PixelDiffReason::PixelChange,
                intensity: Some(0.4),
            },
        ];

        // Standard clustering merges them (spatially adjacent)
        let standard_config = ClusteringConfig {
            gap_threshold: 0.2,
            min_cluster_size: 1,
        };
        let standard = cluster_regions(&regions, &standard_config);
        assert_eq!(standard.len(), 1, "standard clustering merges adjacent regions");

        // Image-aware clustering keeps them separate (different backgrounds)
        let ia_config = ImageAwareClusteringConfig {
            gap_threshold: 0.2,
            color_similarity_threshold: 0.95,
            sample_count: 16,
        };
        let image_aware = cluster_regions_image_aware(&regions, &dynamic_img, &ia_config);
        assert_eq!(
            image_aware.len(),
            2,
            "image-aware clustering separates regions with different background colors"
        );
    }
}
