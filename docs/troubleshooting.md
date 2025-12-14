# Troubleshooting

Common issues and quick fixes when running DPC locally or in CI.

## crates.io / offline failures
- Symptom: `cargo test`/`cargo check` fails to fetch crates (e.g., “Could not resolve host: index.crates.io”).
- Fix: retry with network; offline runs only work after dependencies are cached locally. There is no vendored crate cache.

## Playwright / browser missing
- Symptom: URL rendering fails; errors mention Node/Playwright/Chromium missing.
- Fix: install Node, then `npm install` (once package.json lands) and `npx playwright install chromium`. For tests or CI without a browser, set mocks: `DPC_MOCK_RENDER_REF` and `DPC_MOCK_RENDER_IMPL` (or `DPC_MOCK_RENDERERS_DIR`) to point at local PNGs.

## Figma token missing
- Symptom: Figma normalization errors; status 401/403 or “FIGMA_TOKEN required”.
- Fix: set `FIGMA_TOKEN` (or `FIGMA_OAUTH_TOKEN`) in the environment. For offline/CI without Figma, use mock render vars to bypass Figma calls.

## Missing input files
- Symptom: compare exits with code 2 and category `config`; message includes “File not found”.
- Fix: ensure paths are correct/accessible; relative paths are resolved from the current working directory.

## Threshold failures vs fatal errors
- Exit 1: similarity below threshold (validation failure).
- Exit 2: config/network/runtime errors (treat as infra issues).

## Keeping artifacts for debugging
- By default, artifacts go under the OS temp dir as `dpc-<pid>-<timestamp>/` (e.g., `/tmp/dpc-1234-1700000000000/`) and are cleaned up.
- Use `--keep-artifacts` (or `--artifacts-dir`) and upload the directory in CI for investigation.
