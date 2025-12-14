# Reporting & Outputs

Reference for what the CLI prints and how to consume it in pipelines.

## Compare output
- Schema: versioned via `DPC_OUTPUT_VERSION` (currently `0.2.0`) in `dpc_lib::output`. Fields and naming are stable across JSON and pretty.
- Format: `json` (default) or `pretty`.
- Behavior:
  - On a TTY with no `--output`, `pretty` renders a human-friendly summary (status badge, similarity vs threshold, top issues, metrics, artifacts).
  - When piping or using `--output`, both `json` and `pretty` emit JSON (pretty-printed when `pretty` is chosen) so pipelines stay stable.
- Fields (JSON): `mode` (`compare`), `version`, `ref_resource`, `impl_resource`, `viewport`, `similarity`, `threshold`, `passed`, `metrics` (pixel/layout/typography/color/content), `summary.top_issues`.
- Pretty (TTY) example:
```
PASS Design parity check
Similarity: 96.5% (threshold 95.0%)
Top issues (max 5):
- Design parity check passed (96.5% similarity, threshold: 95.0%)
Metrics:
- pixel         0.965
- layout        0.981
- typography    0.942
- color         0.973
- content       0.955
Artifacts:
- directory      /tmp/dpc-1234-1700000000000
- refScreenshot  /tmp/dpc-1234-1700000000000/ref.png
- implScreenshot /tmp/dpc-1234-1700000000000/impl.png
```

## Errors
- Serialized as `{ "category": "<config|network|figma|image|metric|unknown>", "message": "...", "remediation": "..."? }` in JSON mode; pretty prints category and hint to stdout (or file if `--output` is set).
- Common causes: missing Playwright (`npm install playwright && npx playwright install chromium`), missing `FIGMA_TOKEN` / `node-id` on Figma URLs, invalid viewport (`WIDTHxHEIGHT`), unsupported image extension, timeouts (raise `--nav-timeout` / `--network-idle-timeout`).

## Exit codes
- `0`: compare passed (similarity >= threshold) or stub commands succeeded.
- `1`: compare failed threshold.
- `2`: configuration/network/runtime errors.

## Artifacts
- Written under the OS temp dir as `dpc-<pid>-<timestamp>/` (e.g., `/tmp/dpc-1234-1700000000000/`) by default: `ref_screenshot.png`, `impl_screenshot.png`, DOM snapshots, and Figma exports.
- Use `--keep-artifacts` (or `--artifacts-dir`) to retain; otherwise cleaned up after compare. Output includes an `artifacts` block with paths:
```json
{
  "artifacts": {
    "directory": "artifacts/run1",
    "kept": true,
    "refScreenshot": "artifacts/run1/ref_screenshot.png",
    "implScreenshot": "artifacts/run1/impl_screenshot.png",
    "diffImage": "artifacts/run1/diff_heatmap.png",
    "refDomSnapshot": "artifacts/run1/ref_dom.json",
    "implDomSnapshot": "artifacts/run1/impl_dom.json",
    "refFigmaSnapshot": null,
    "implFigmaSnapshot": null
  }
}
```
- Mocking (offline/CI): set `DPC_MOCK_RENDER_REF` / `DPC_MOCK_RENDER_IMPL` to PNGs, or `DPC_MOCK_RENDERERS_DIR=/path` containing `ref.png` / `impl.png`. Mocking only applies to URL/Figma kinds.

## Metrics weights
- Combined score weights: pixel 0.35, layout 0.25, typography 0.15, color 0.15, content 0.10. Only available metrics contribute.

## Figma & browser notes
- Figma: requires `FIGMA_TOKEN`; URLs must include `node-id`.
- Browser: Node on PATH + Playwright package; headless by default; waits for navigation and `networkidle`.

## Integration tips
- Prefer JSON for automation; pretty for humans.
- Treat exit code 1 as a validation failure (non-fatal) and 2 as infra/config to surface loudly.
- Preserve artifacts in CI when debugging by adding `--keep-artifacts` and uploading the temp dir. 
- For CLI examples (ignore-regions JSON, artifacts-dir usage), see `docs/cli_usage.md`.
