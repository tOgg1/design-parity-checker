# CI Cheatsheet (local & pipeline)

Quick reference for running and interpreting the CI steps, with tips for local dry-runs and mocking external dependencies.

## Core commands (mirrors GitHub Actions)
- Check: `cargo check --all-targets --locked`
- Test: `cargo test --all-targets --locked`
- Format: `cargo fmt --all --check`
- Clippy: `cargo clippy --all-targets --locked -- -D warnings`
- Release build: `cargo build --release --locked`

## Exit codes (compare)
- `0`: passed (similarity >= threshold)
- `1`: threshold failed (non-fatal validation)
- `2`: fatal/config/network errors

## Mocking to avoid external deps
- URL/Figma rendering: set `DPC_MOCK_RENDER_REF` and `DPC_MOCK_RENDER_IMPL` to local PNGs, or `DPC_MOCK_RENDERERS_DIR=/path` containing `ref.png` / `impl.png`.
- Figma token: real runs need `FIGMA_TOKEN` (or `FIGMA_OAUTH_TOKEN`). Mocks bypass Figma calls.
- Playwright: real URL rendering needs Node + `npx playwright install chromium`. Mocks bypass the browser.

## Artifacts
- Compare writes temp artifacts under the OS temp dir as `dpc-<pid>-<timestamp>/` (e.g., `/tmp/dpc-1234-1700000000000/`) by default (screenshots/DOM/Figma exports).
- Use `--keep-artifacts` or `--artifacts-dir` to retain and upload in CI for debugging; otherwise they are cleaned up.
- Output includes an `artifacts` block with the directory and file paths (screenshots, DOM/Figma snapshots, optional `diff_heatmap.png`) when artifacts are kept.

## Local dry-run tips
- Run `cargo test --test compare_integration` to validate exit codes and output shapes. With mocks set, no browser/Figma calls occur.
- If crates.io is unreachable, `--locked` will fail; rerun with network available. There’s no vendored crate cache.

## Pipeline notes
- Keep commands `--locked` to ensure Cargo.lock fidelity.
- Treat exit code `1` as a validation failure (should block merge but not mark infra flaky); treat `2` as infra/config (surface loudly/retry).
- Cache: GitHub Actions uses `Swatinem/rust-cache`; no special config needed locally.
