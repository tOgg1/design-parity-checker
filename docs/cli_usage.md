# CLI Usage Cheatsheet

Commands:
- `dpc compare --ref <resource> --impl <resource> [--ref-type/--impl-type] [--viewport WxH] [--threshold FLOAT] [--metrics list] [--ignore-selectors ".ads,#banner"] [--ignore-regions regions.json] [--format json|pretty] [--output PATH] [--keep-artifacts|--artifacts-dir PATH]`
- `dpc generate-code --input <resource> [--stack html+tailwind] [--viewport WxH] [--output PATH] [--format json|pretty]` (stub)
- `dpc quality --input <resource> [--viewport WxH] [--output PATH] [--format json|pretty]` (stub)

Global flags:
- `--config <PATH>`: optional TOML to set defaults (viewport, threshold, metric weights, timeouts) when wiring is enabled.
- `--verbose`: prints basic progress.

Key options:
- `--viewport`: default `1440x900`.
- `--threshold`: default `0.95` for compare.
- `--metrics`: comma list of `pixel,layout,typography,color,content`; if omitted, all available metrics run (pixel+color when no DOM/figma).
- `--ignore-selectors`: comma-separated CSS selectors to drop DOM nodes before structural metrics.
- `--ignore-regions`: JSON array of `{x,y,width,height}` rectangles to mask before pixel/color metrics. A ready-made full-frame mask lives at `test_assets/ignore_regions_example.json`.
- `--keep-artifacts` or `--artifacts-dir`: retain screenshots/DOM/Figma exports; artifacts block surfaces in output so downstream jobs can consume them. Default temp dir lives under the OS temp folder as `dpc-<pid>-<timestamp>/` and is removed when neither flag is set.
- Timeouts: `--nav-timeout` (default 30s), `--network-idle-timeout` (default 10s), `--process-timeout` (default 45s).

Outputs:
- `--format json|pretty`: on a TTY with no `--output`, `pretty` renders a human-readable summary (status badge, similarity, top issues, metrics, artifacts). When piping or using `--output`, both formats emit JSON; `pretty` pretty-prints JSON for readability while keeping schema identical.
- Exit codes: 0 pass / stub success; 1 threshold fail; 2 errors.
  - Error remediation hints are included (e.g., install Playwright/Chromium, set FIGMA_TOKEN and node-id, check image extension, raise timeouts).
 - Artifacts block (when `--keep-artifacts` or `--artifacts-dir` is used) surfaces the directory plus paths to screenshots, DOM/Figma snapshots, and optional diff heatmap:
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

Resources:
- Auto-detected: url | image | figma; override with `--*-type`.
- Figma requires `FIGMA_TOKEN` and `node-id` in the URL.
- URL rendering requires Node + Playwright + Chromium download.

## Examples
- Image vs image (JSON):  
  `dpc compare --ref ref.png --impl impl.png --threshold 0.95 --format json`
- URL vs URL with mocks (no browser hit):  
  `DPC_MOCK_RENDER_REF=ref.png DPC_MOCK_RENDER_IMPL=impl.png dpc compare --ref https://design --impl https://build --format pretty`
- Figma vs image (needs FIGMA_TOKEN):  
  `FIGMA_TOKEN=... dpc compare --ref https://www.figma.com/file/FILE/Design?node-id=1-2 --impl impl.png --ref-type figma --format json --keep-artifacts`
- Ignore regions (mask pixel/color):  
  `dpc compare --ref ref.png --impl impl.png --ignore-regions regions.json --format json`  
  `regions.json` is an array of `{x,y,width,height}` (or `w,h`), values in px or 0–1 (percent of viewport). Example full-frame mask: `[{"x":0,"y":0,"w":1,"h":1}]`.
- Keep artifacts in custom dir:  
  `dpc compare --ref ref.png --impl impl.png --artifacts-dir artifacts/run1 --threshold 0.9 --format pretty`  
  Artifacts (screenshots/DOM/Figma) are retained under the chosen directory.
- CI/piped run (JSON stable even with `pretty`):  
  `dpc compare --ref https://design --impl https://build --format pretty --output results.json --artifacts-dir artifacts/run2`
- TTY human summary (no output file):  
  `dpc compare --ref https://design --impl https://build --format pretty`
- Generate code (stub output):  
  `dpc generate-code --input https://www.figma.com/file/FILE/Design?node-id=1-2 --stack html+tailwind --format json`
- Quality (experimental stub):  
  `dpc quality --input impl.png --format pretty`
