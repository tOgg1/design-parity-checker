# Metrics Overview

This doc summarizes the implemented metrics to help with testing and expected behaviors. It covers inputs, thresholds, and what “diffs” mean for each metric.

## Pixel (SSIM-style)
- Loads reference/implementation screenshots; resizes implementation to reference dimensions if needed.
- Computes SSIM-like score on luma.
- Diffs: image is split into blocks (default 32px). Average per-block diff is classified with thresholds (minor ≥0.05, moderate ≥0.15, major ≥0.3). Regions include normalized x/y/width/height and severity.
- Score: 0..1, higher is better.

## Layout
- Uses structural data (DOM or Figma). Each node is typed (button, heading, text, image, input, other) and compared via IoU.
- Matching: elements of the same kind are matched if IoU exceeds `match_threshold` (0.1) and `iou_threshold` (0.5). Unmatched refs → MissingElement; unmatched impl → ExtraElement; mismatched positions/sizes → PositionShift/SizeChange.
- Score: proportion of matched elements, 0..1. Diffs list kind, element_type label, and normalized bbox.

## Typography
- Requires text nodes with computed_style/typography. Compares family (canonicalized), size, weight, line-height with tolerances: size diff penalized proportionally; weight and line-height penalized if they differ beyond tolerance.
- Issues per text node: FontFamilyMismatch, FontSizeDiff, FontWeightDiff, LineHeightDiff. Penalties combine into a score 0..1.

## Color Palette
- Samples pixels (stride) and runs k-means to get palette (cluster count bounded by samples). Computes match score by nearest-colors distance (deltaE-like) weighted by reference palette shares.
- Diffs: top palette colors reported as Primary/Accent/Background color shifts with hex values and optional delta.
- Score: 0..1.

## Content
- Extracts text from DOM, Figma nodes, and OCR blocks (if present). Normalizes text (lowercase, alnum + spaces) and compares sets.
- Missing text (present in ref, absent in impl) and extra text (present in impl, absent in ref) are recorded; score penalized for these counts.
- Score: 0..1; diffs list missing_text and extra_text strings.

## Combined score & defaults
- Default metrics: Pixel, Layout, Typography, Color, Content.
- Combined score weights (default): pixel 0.35, layout 0.25, typography 0.15, color 0.15, content 0.10. Only present metrics are renormalized.
- If no structural data (no DOM/Figma) is available, run_metrics automatically skips layout/typography/content and keeps pixel+color.

## Refactor plan (bead 9iw)
- Target structure under `src/metrics/`: `mod.rs` (re-exports), `pixel.rs`, `layout.rs`, `typography.rs`, `color.rs`, `content.rs`, `weights.rs`, `issues.rs` (shared issue structures), `utils.rs` (palette/sample helpers), `top_issues.rs` (summary generator).
- Migration steps (incremental, non-breaking):
  1) Extract shared types/helpers first (`weights`, `issues`, `top_issues`, `utils`) and re-export from `mod.rs` while keeping call sites unchanged.
  2) Move each metric into its file, re-exporting the same public types/functions (`run_pixel_metric`, `PixelMetric`, etc.) to avoid large call-site churn.
  3) Keep `run_metrics` orchestration in `mod.rs` until all metrics are moved, then split orchestration if needed.
- Acceptance guardrails: preserve public API used by `lib.rs` re-exports; run full test suite after each extraction; avoid behavioral changes while moving code.
