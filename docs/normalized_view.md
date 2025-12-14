# NormalizedView & Input Pipeline

This doc describes how inputs become a `NormalizedView`, what fields exist, and where artifacts land.

## Resource kinds
- **Image**: Local files (png/jpg/jpeg/webp/gif). Loaded and letterboxed to viewport via `image_loader`.
- **URL**: Rendered with Node + Playwright, waits for navigation + `networkidle`, captures screenshot and DOM (with computed styles: font, color, display, visibility, opacity).
- **Figma**: Uses REST export for the specified `file_key` + `node-id` (requires `FIGMA_TOKEN`). Exports PNG and maps the node tree to `NormalizedView`.

## NormalizedView fields
- `kind`: `Url | Image | Figma`.
- `screenshot_path`: PNG written to the artifacts dir.
- `width/height`: Viewport used for normalization.
- `dom`: Optional DOM snapshot (URL) with nodes (id/tag/children/attrs/text/bounding_box/computed_style).
- `figma_tree`: Optional Figma node tree (frames, text nodes, fills, typography).
- `ocr_blocks`: Reserved for future OCR (currently unused).

## Where artifacts go
- Compare writes under the OS temp dir as `dpc-<pid>-<timestamp>/` by default (e.g., `/tmp/dpc-1234-1700000000000/`): `ref_screenshot.png`, `impl_screenshot.png`, DOM snapshots, and Figma exports.
- `--keep-artifacts` retains the directory; otherwise it is cleaned up after compare.
- Mocking (offline/CI): `DPC_MOCK_RENDER_REF` / `DPC_MOCK_RENDER_IMPL` PNGs, or `DPC_MOCK_RENDERERS_DIR=/path` with `ref.png` / `impl.png` (applies to URL/Figma kinds).

## Pipelines
- **URL → NormalizedView**: `url_to_normalized_view` (Playwright) produces screenshot + DOM. Browser defaults: headless, navigation 20s, network idle 5s, process timeout 45s.
- **Image → NormalizedView**: `image_to_normalized_view` resizes/letterboxes to viewport; no DOM/figma tree.
- **Figma → NormalizedView**: `figma_to_normalized_view` exports the target node to PNG (respecting viewport/scale) and builds a node tree; needs `FIGMA_TOKEN` and `node-id`.

## Metrics expectations
- Pixel/color work for any kind.
- Layout/typography/content require structure (DOM or figma_tree).
- When both inputs lack structure, compare defaults to pixel+color only.
