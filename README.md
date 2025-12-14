# Design Parity Checker (DPC)

CLI tool to measure how closely an implementation matches a reference design (Figma, URL, or image), surface differences, and optionally generate code.

## Current state
- `compare` runs end-to-end for URL, image, and Figma inputs: renders, normalizes, executes metrics, and reports pass/fail.
- `generate-code` and `quality` are **placeholders** that return structured `not_implemented` summaries with exit code 0.
- Metrics implemented: pixel, layout, typography, color, content (see `src/metrics.rs`).
- Pretty output: interactive TTY runs render a human-readable summary (PASS/FAIL badge, similarity, top issues, metrics, artifact paths). When piping or using `--output`, even `--format pretty` emits JSON (pretty-printed) to keep pipelines stable.

## Install
```bash
cargo install --path .
# For URL rendering: ensure Node is on PATH, then install Playwright + Chromium
npm install playwright
npx playwright install chromium
```
Ensure `FIGMA_TOKEN` (or `FIGMA_OAUTH_TOKEN`) is set if you will process Figma URLs.

## CLI usage
For a concise CLI reference with examples, see `docs/cli_usage.md` (includes ignore-regions and artifacts examples, plus a ready-made mask at `test_assets/ignore_regions_example.json`).

Artifacts (screenshots/DOM/Figma snapshots) are kept when you pass `--keep-artifacts` or `--artifacts-dir` (implies keep). Ignore regions (`--ignore-regions regions.json`) accept JSON rectangles `{x,y,width,height}` (or `w,h`), using px or 0–1 normalized to the viewport; masking applies before pixel/color metrics.

### compare
```
dpc compare --ref <resource> --impl <resource> \
  [--ref-type url|image|figma] [--impl-type ...] \
  [--viewport WIDTHxHEIGHT] [--threshold FLOAT] \
  [--metrics pixel,layout,typography,color,content] \
  [--ignore-selectors ".ads,#cookie-banner"] \
  [--format json|pretty] [--output PATH] [--keep-artifacts] [--artifacts-dir PATH]
```
- Resources auto-detect type; override with `--ref-type/--impl-type`.
- Viewport default: `1440x900`. Threshold default: `0.95`.
- Metrics: if omitted, all available metrics run; when both inputs lack DOM, defaults to pixel+color only.
  - DOM ignores: `--ignore-selectors` drops matching nodes (id/class/tag) before structural metrics. `--ignore-regions` accepts a JSON array of `{x,y,width,height}` (aliases `w`/`h` ok) to mask before pixel/color metrics; coordinates apply to the normalized viewport (e.g., 1440x900), and values between 0–1 are treated as percentages of the viewport so you can cover the full frame with `{x:0,y:0,w:1,h:1}`. Invalid/empty files exit with code 2. See `test_assets/ignore_regions_example.json` for a ready-made full-frame mask.
- Artifacts: stored under the OS temp dir as `dpc-<pid>-<timestamp>/` (e.g., `/tmp/dpc-1234-1700000000000/`); `--keep-artifacts` (or `--artifacts-dir`) retains screenshots, diff heatmap (`diff_heatmap.png`), and saves DOM/Figma snapshots as JSON. Use `--artifacts-dir` to choose the folder; paths are echoed to stderr (with per-file details in `--verbose`).
- Mock rendering (useful in CI/offline): set `DPC_MOCK_RENDER_REF` / `DPC_MOCK_RENDER_IMPL` to PNG paths, or `DPC_MOCK_RENDERERS_DIR=/path` containing `ref.png` / `impl.png`.
- Output shape: on a TTY with no `--output`, `--format pretty` renders the human summary; with `--output` or when piped, both `json` and `pretty` produce JSON (pretty-printed when `pretty` is chosen).

Example:
```
dpc compare --ref https://example.com/design \
            --impl https://example.com/build \
            --viewport 1366x768 \
            --format pretty
```

### generate-code (stub)
```
dpc generate-code --input <resource> [--stack html+tailwind] [--viewport WIDTHxHEIGHT] [--output PATH]
```
Returns a `not_implemented` summary; exit code 0.

### quality (stub)
```
dpc quality --input <resource> [--viewport WIDTHxHEIGHT] [--format json|pretty]
```
Returns a `not_implemented` finding; exit code 0.

## Inputs and normalization
- Resource kinds: `url`, `image`, `figma` (auto-detected).
- Images: loaded and letterboxed to viewport (`src/image_loader.rs`).
- URLs: rendered headless via Node + Playwright; waits for navigation then `networkidle`, captures screenshot and DOM (incl. computed styles).
- Figma: uses REST API to export the specified node; requires `FIGMA_TOKEN` and `node-id` in the URL query.
- Ignore regions example (`--ignore-regions regions.json`):
  ```json
  [
    {"x": 0, "y": 0, "width": 200, "height": 100},
    {"x": 400, "y": 300, "width": 150, "height": 120}
  ]
  ```
  Regions are masked (black) in both ref/impl before pixel/color metrics. Values between 0–1 are treated as percentages of the viewport (`{x:0,y:0,w:1,h:1}` ignores the full frame).

## Troubleshooting (exit code 2)
- Playwright/Chromium missing: `npm install playwright` and `npx playwright install chromium`.
- Node not on PATH: install Node.js and ensure `node` is discoverable.
- Figma inputs: set `FIGMA_TOKEN`, include `?node-id=...`, and use a valid Figma file URL.
- Timeouts: raise `--nav-timeout` / `--network-idle-timeout` / `--process-timeout` or unblock slow pages.
- Missing/unsupported file: use an absolute path and a supported image (png, jpg, jpeg, webp, gif) or override via `--ref-type/--impl-type`.

## Outputs and schemas
- All CLI responses share a tagged schema (`mode`, `version`) defined in `DpcOutput` (`DPC_OUTPUT_VERSION` is `0.2.0`). `--format pretty` is the same JSON, pretty-printed.
- Success payload (compare) example:
```json
{
  "mode": "compare",
  "version": "0.2.0",
  "ref": {"kind": "url", "value": "https://ref"},
  "impl": {"kind": "image", "value": "impl.png"},
  "viewport": {"width": 1440, "height": 900},
  "similarity": 0.97,
  "threshold": 0.95,
  "passed": true,
  "metrics": {...},
  "summary": {"topIssues": ["Design parity check passed (97.0% similarity, threshold: 95.0%)"]},
  "artifacts": {
    "directory": "/tmp/dpc-1234-1700000000000",
    "kept": true,
    "refScreenshot": "/tmp/dpc-1234-1700000000000/ref_screenshot.png",
    "implScreenshot": "/tmp/dpc-1234-1700000000000/impl_screenshot.png",
    "diffImage": null,
    "refDomSnapshot": "/tmp/dpc-1234-1700000000000/ref_dom.json",
    "implDomSnapshot": "/tmp/dpc-1234-1700000000000/impl_dom.json",
    "refFigmaSnapshot": null,
    "implFigmaSnapshot": null
  }
}
```
- Error payload shape (also pretty-printed for `--format pretty`; both formats write to stdout unless `--output` is set):
```json
{
  "mode": "error",
  "version": "0.2.0",
  "error": {
    "category": "config",
    "message": "File not found: missing.png",
    "remediation": "Check file paths/permissions."
  }
}
```
- Error payloads are printed to stdout for both `json` and `pretty` formats (or written to `--output` if provided).
- `artifacts` is present when `--keep-artifacts` or `--artifacts-dir` is used; fields include `directory`, `kept`, `refScreenshot`, `implScreenshot`, optional `diffImage`, `refDomSnapshot`, `implDomSnapshot`, `refFigmaSnapshot`, `implFigmaSnapshot`.
- Human pretty output (TTY-only) mirrors these fields as a compact, colored summary for interactive use; JSON remains stable for piping/CI.
- Schema location: see `dpc_lib::output` (e.g., `src/lib.rs` types) for the authoritative Rust structs defining the JSON fields.

## Metrics
- Pixel: diff score plus diff regions.
- Layout: missing/extra/shifted elements (requires DOM/figma tree).
- Typography: font family/size/line-height comparisons (requires text nodes).
- Color: palette distance and mismatches.
- Content: missing/extra text blocks.
- Combined score uses weights pixel 0.35, layout 0.25, typography 0.15, color 0.15, content 0.10 (see `ScoreWeights`).

## Exit codes
- `0`: compare passed (similarity >= threshold), or stub commands succeeded.
- `1`: compare failed threshold.
- `2`: configuration/network/runtime errors.

## Configuration & timeouts
- Browser defaults: navigation 30s, network idle 10s, process timeout 45s, headless on. Verbose mode logs capture stages (launch, navigate, network-idle, capture).
- Playwright requires the `playwright` npm package and a Chromium download (`npx playwright install chromium`).
- Figma requires `FIGMA_TOKEN`; `node-id` must be present for the target frame/node.
- Optional config file: `--config dpc.toml` sets defaults for viewport, threshold, metric weights, and timeouts. CLI flags override config when provided. Example:
  ```toml
  viewport = "1280x720"
  threshold = 0.9
  [metric_weights]
  pixel = 0.4
  layout = 0.2
  typography = 0.15
  color = 0.15
  content = 0.1
  [timeouts]
  navigation = "20s"
  network_idle = "8s"
  process = "45s"
  ```
- When `--verbose` is set, compare logs the effective config (source, viewport, threshold, weights, timeouts) before rendering.

## Build & test
```bash
cargo build
cargo test
cargo clippy --all-targets --all-features
```
URL/Figma tests may require Node/Playwright/FIGMA_TOKEN; use mock env vars for offline runs.

## CI integration
- Recommended flags: `--format json` (or `--format pretty --output results.json`), plus `--artifacts-dir` to persist screenshots and DOM snapshots for uploads.
- Exit codes are CI-friendly: `0` pass/stub success, `1` threshold fail, `2` configuration/runtime errors.
- Typical steps:
  1. Install deps (`npm install playwright && npx playwright install chromium` if comparing URLs).
  2. Export FIGMA_TOKEN for Figma flows.
  3. Run `dpc compare ... --format json --artifacts-dir artifacts/` and archive the `artifacts/` folder.
  4. Parse `results.json` for similarity/metrics in downstream jobs.

## Troubleshooting
- “Cannot find module 'playwright'”: run `npm install playwright` and `npx playwright install chromium`.
- Viewport must be `WIDTHxHEIGHT` (e.g., `1440x900`).
- Unsupported images: ensure file exists and extension is png/jpg/jpeg/webp/gif.
- Figma: ensure `FIGMA_TOKEN` is set and the URL includes `node-id`.
- Ignore regions: coordinates map to the normalized viewport. If your inputs are scaled up from tiny fixtures, use a region at least as large as the viewport to fully mask differences.
- Remediation hints: CLI errors include suggested fixes (Playwright install, FIGMA_TOKEN/node-id, image extension, timeouts). Exit code `2` indicates configuration/runtime, not similarity.

## Coordination / agent workflow
- Claim a bead (`bd update <id> --status in_progress`), reserve files before edits, and announce via Agent Mail.
- Keep `.beads/` in sync with code changes; do not delete files without explicit approval.
- Share the commands/tests you ran and release reservations when handing off; include artifact paths when relevant.
- For stale work, force-release reservations via Agent Mail tooling (with a note), then proceed after announcing to the team.

## License
MIT
