# Design Parity Checker (DPC)

CLI tool to measure how closely an implementation matches a reference design (Figma, URL, or image), surface differences, and optionally generate code.

## Project state (in progress)
- CLI argument parsing, resource detection, viewport parsing, output schemas, and metric/result types are implemented.
- Capture/metrics pipelines are **stubbed**: commands return structured placeholder outputs until browser/Figma capture and metrics are wired.
- Browser capture (Node + Playwright) and Figma normalization are underway; image normalization is functional.

## Commands (current behavior: stubs)
- `dpc compare --ref <resource> --impl <resource> [--viewport WIDTHxHEIGHT] [--threshold FLOAT] [--metrics list] [--format json|pretty]`
- `dpc generate-code --input <resource> [--stack html+tailwind] [--viewport WIDTHxHEIGHT] [--output PATH]`
- `dpc quality --input <resource> [--viewport WIDTHxHEIGHT] [--format json|pretty]`

All commands currently emit JSON/pretty JSON placeholders; metrics/capture are not yet executed.

### Flags and defaults
- `--viewport` default: `1440x900`, validated as `WIDTHxHEIGHT`.
- `--threshold` default: `0.95` (compare).
- `--format`: `json` (compact) or `pretty` (indented).
- `--verbose`: prints basic banners; detailed staging will come with capture.

## Inputs and normalization
- Resource kinds: `url`, `image`, `figma` (auto-detected; override with `--ref-type/--impl-type/--input-type`).
- Images: loaded via `image_loader.rs` (letterbox resize helpers).
- Figma: `figma.rs`/`figma_client.rs` handle tokenized API access, node export, and mapping to `NormalizedView` (requires `FIGMA_TOKEN` or `FIGMA_OAUTH_TOKEN`).
- Browser/URL: Node Playwright helper (WIP) will render, wait for network idle, capture screenshot/DOM. Node + `playwright` package will be required once enabled.

## Outputs
`DpcOutput` (tagged by `mode`) covers:
- Compare: similarity, threshold, per-metric scores/diffs, summary (top issues), resources, viewport.
- GenerateCode: input descriptor, optional viewport/stack/output path, generated code + summary.
- Quality: heuristic score plus findings (severity, type, message).

Schemas live in `src/output.rs` and are re-exported from the library.

## Metrics (library types)
Types exist for Pixel, Layout, Typography, Color, and Content metrics with diff regions. Implementations are in `src/metrics.rs`; pixel/layout/typography/color/content logic is present but not yet invoked by CLI stubs. Palette math uses `palette` crate; typography helpers normalize labels and compare size/line-height with tolerances.

## Prerequisites
- Rust toolchain, cargo.
- For upcoming browser capture: Node + `playwright` (Chromium download).
- Figma access: set `FIGMA_TOKEN` or `FIGMA_OAUTH_TOKEN`.

## Build & test
```bash
cargo build
cargo test   # may require network for crate/index; blocked in some envs
cargo clippy --all-targets --all-features
```
If crates.io is unreachable, tests will fail to fetch dependencies; retry with network available.

## Exit codes
- `0`: compare passed (similarity >= threshold)
- `1`: compare failed threshold
- `2`: fatal/config/network/processing errors (all error paths)

CI usage: treat `1` as a validation failure (threshold miss) and `2` as an infrastructure/config error that should surface as a hard failure or retry. Errors are emitted in the selected output format (JSON or pretty) so pipelines can capture structured payloads.

## Troubleshooting
- Viewport must be `WIDTHxHEIGHT` (e.g., `1440x900`).
- Figma URLs must include a file key; node-id is recommended for frames.
- Image inputs must exist and use supported extensions: png, jpg, jpeg, webp, gif.
- Nested output paths for normalized images are now created automatically.
- If browser capture fails: ensure Node + `playwright` installed and Chromium downloaded; headless is default.

## Roadmap highlights (per product spec)
- Full compare pipeline: URL/Figma render → normalize → metrics → summary.
- Generate-code: HTML+Tailwind MVP from screenshots/figma.
- Quality mode: heuristic findings (alignment/spacing/contrast/hierarchy) marked experimental.
- Reporting: JSON/pretty outputs; artifact paths for screenshots/DOM/figma exports.

## Coordination
- File reservations are enforced; please reserve before editing shared files (src/*, Cargo.toml) and release promptly.

## License
MIT
