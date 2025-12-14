# Developer Setup

This project is a Rust CLI that depends on optional Node/Playwright and Figma access. Use this guide to get a working local environment and to run tests reliably (including offline constraints).

## Prerequisites
- Rust toolchain (stable) with `cargo`.
- Node + npm (only needed for Playwright/browser capture work).
- Optional: Chromium downloaded via `npx playwright install chromium` if you plan to run URL rendering.
- Figma: set `FIGMA_TOKEN` (or `FIGMA_OAUTH_TOKEN`) for real Figma calls.

## Environment variables
- `FIGMA_TOKEN` / `FIGMA_OAUTH_TOKEN`: required for live Figma normalization.
- `DPC_MOCK_RENDER_REF` / `DPC_MOCK_RENDER_IMPL`: point URL/Figma inputs to local PNGs to avoid Playwright/Figma in tests.
- `DPC_MOCK_RENDERERS_DIR`: alternative to the two env vars; set a directory containing `ref.png` / `impl.png`.
- `CARGO_BIN_EXE_dpc`: set automatically by `cargo test` when invoking integration tests; not needed manually.

## Running tests
```bash
cargo test --all-targets      # runs unit + integration tests
```
- If crates.io is unreachable, tests will fail to fetch dependencies. Re-run with network available; offline `cargo test` only works once dependencies are cached locally.
- Integration tests for compare use the mock render env vars above to avoid external services.

## Playwright/browser notes
- URL rendering depends on Node and Playwright’s Chromium. Install once: `npm install` (when package.json lands) then `npx playwright install chromium`.
- In CI, ensure Chromium is available or use the mock render env vars to avoid real browser calls.

## Artifacts and paths
- Temporary artifacts (screenshots/DOM) are written under the OS temp dir as `dpc-<pid>-<timestamp>/` (e.g., `/tmp/dpc-1234-1700000000000/`) unless `--keep-artifacts` is passed with a custom path.
- Normalized screenshots for image inputs are saved where you point the output path; parent directories are created automatically.

## Common issues
- Missing image paths → exit code 2 with `category: "config"`.
- Threshold miss → exit code 1.
- Pass → exit code 0.
- When offline, registry fetches will fail; retry with connectivity. There is no vendored crate cache in the repo.
