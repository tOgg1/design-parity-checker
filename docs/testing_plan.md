# Testing Plan

Practical steps to validate the CLI today, with and without external deps.

## Foundational
- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test -- --nocapture`

## Core CLI flows (no browser/Figma; use bundled fixtures/mocks)
1) Image parity (JSON)  
   `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --format json`
2) Pretty TTY vs piped/file JSON  
   - TTY: `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --format pretty`  
   - Piped/file: `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --format pretty --output out.json`
3) Threshold fail exit code  
   `dpc compare --ref test_assets/ref.png --impl test_assets/big_diff.png --threshold 0.99`
4) Ignore selectors (DOM filter)  
   `dpc compare --ref test_assets/ref_dom.png --impl test_assets/impl_dom.png --ignore-selectors ".ads,#cookie"`
5) Ignore regions (pixel mask)  
   `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --ignore-regions test_assets/ignore_regions_example.json`
6) Keep artifacts  
   `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --keep-artifacts --artifacts-dir artifacts/run1`
7) Config file + flag precedence (use --verbose to see effective config)  
   `dpc compare --ref test_assets/ref.png --impl test_assets/impl.png --config dpc.toml --process-timeout 50 --format json --verbose`

## Stub commands (UX sanity)
- Generate code stub: `dpc generate-code --input test_assets/ref.png --format pretty`
- Quality stub: `dpc quality --input test_assets/ref.png --format json`

## Network/browser (Playwright/Node required)
Prereq: `npm install playwright && npx playwright install chromium`
- URL vs URL: `dpc compare --ref https://example.com --impl https://example.org --format json --verbose`
- Timeout behavior: `dpc compare --ref https://example.com --impl https://example.org --nav-timeout 5 --network-idle-timeout 3`

## Figma (token + node-id required)
Prereq: export `FIGMA_TOKEN`
- `dpc compare --ref "https://www.figma.com/file/<KEY>/Design?node-id=1-2" --impl test_assets/impl.png --ref-type figma --format json --keep-artifacts`
- Negative cases: missing token or node-id → expect remediation in output.

## Mocked Figma/URL (no network)
- `DPC_MOCK_RENDER_REF=/path/ref.png DPC_MOCK_RENDER_IMPL=/path/impl.png dpc compare --ref https://ref --impl https://impl --format pretty`
- `DPC_MOCK_RENDERERS_DIR=/path/dir_with_ref_impl dpc compare --ref https://ref --impl https://impl --format json`

## Regression focus
- Pretty vs JSON parity: `--format pretty --output file` still JSON.
- Artifacts block only when keep/artifacts-dir set; paths valid.
- Error remediation text: Playwright install, FIGMA_TOKEN/node-id, unsupported extension, timeouts.

## What you need
- Rust toolchain (already used by `cargo test`).
- For browser runs: Node + Playwright + Chromium download.
- For Figma runs: `FIGMA_TOKEN` and a URL with `node-id`.
- You can start today with the mock-based suite + full `cargo test`; add browser/Figma runs when deps/tokens are available.
