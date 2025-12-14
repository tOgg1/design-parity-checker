# Design Parity Checker (DPC) – MVP Product Spec v0.2

## 1. Purpose

**Design Parity Checker (DPC)** is a CLI and library that answers one core question:

> *“How close is this implementation to the intended design, exactly, and where does it differ?”*

It is meant to be:

* **Agent-friendly**: JSON-only interface, deterministic scores, clear thresholds.
* **Designer-level picky**: catches misaligned buttons, wrong fonts, off-by-1 spacing, wrong colors, missing content.
* **Extensible**: metrics are pluggable and combine into a single similarity score in [0,1].

Recommended semantics:

* `similarity = 1.0` → perfect match (or effectively pixel-perfect).
* `similarity ≥ 0.95` → “good enough parity” for agents / CI.
* `similarity ~0.5` → structurally similar but visually divergent.
* `similarity < 0.2` → essentially unrelated designs.

---

## 2. Scope & Non‑Goals

### 2.1 In Scope (MVP)

* **Parity checks** for:

  * Image ↔ Image
  * URL ↔ URL
  * Figma frame ↔ URL
  * Figma frame ↔ Image

* **Three modes**:

  1. `dpc compare` – reference vs implementation parity.
  2. `dpc generate-code` – minimal HTML/Tailwind from a single input.
  3. `dpc quality` – reference-free heuristic quality score (experimental).

* **Multi-dimensional metrics**:

  * Pixel / perceptual similarity.
  * Layout / structure similarity.
  * Typography similarity.
  * Color similarity.
  * Content similarity.

* **Machine-readable JSON output**:

  * Overall score.
  * Metric breakdown.
  * Diff regions with categories (layout, typography, etc.).

### 2.2 Out of Scope (MVP)

* Responsive / multi-viewport parity (only one viewport per run).
* Animations, transitions, or video.
* Auto-orchestrating agent loops.
* Training custom deep models (we rely on existing metrics + heuristics first).
* IDE / Figma plugins (those can come later, using the CLI/library).

---

## 3. Primary Users & Use Cases

### 3.1 LLM Agent Orchestrators

**Goal:** Use DPC inside an agent loop to enforce “design done-ness”.

Example workflow:

1. Agent generates or edits frontend code.
2. System deploys / runs it in a local dev server.
3. DPC compares `ref` (Figma URL or reference screenshot) vs `impl` (localhost URL).
4. If `similarity >= 0.95`, accept; otherwise agent uses diffs to fix.

### 3.2 Frontend / Design System Engineers

* Run `dpc compare` locally or in CI:

  * “Does this branch still match Figma?”
  * “Did we accidentally break typography or spacing?”

---

## 4. CLI Design

### 4.1 Core Principles

* **Normalized flags**: always `--ref` and `--impl`.
* **Automatic type detection**: infer whether each is a URL, Figma link, or local image.
* **Override when needed**: allow `--ref-type` / `--impl-type` to override detection.
* Paths and URLs should be exactly what humans or agents naturally use.

### 4.2 Resource Type Detection

`--ref` / `--impl` values are parsed as:

1. **HTTP/HTTPS URL** (`^https?://`):

   * If host contains `figma.com`, treat as **Figma**:

     * Support URLs like
       `https://www.figma.com/file/<FILE_KEY>/<FILE_NAME>?type=design&node-id=<NODE_ID>` ([Figma Developer Docs][1])
     * Extract:

       * `FILE_KEY` from `/file/<FILE_KEY>/…`
       * `FRAME_ID` from `node-id` query param.
   * Otherwise treat as a **web page URL** to be rendered via headless browser.

2. **Local path** (no `http(s)://`):

   * If extension is an image format (`.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`):

     * Treat as **image**.
   * (Optionally later: `.html` as local HTML, rendered through a file:// URL.)

3. **Manual override** (optional):

   * `--ref-type image|url|figma`
   * `--impl-type image|url|figma`

   If provided, overrides detection.

### 4.3 Commands

#### 4.3.1 `dpc compare`

**Usage:**

```bash
dpc compare \
  --ref https://www.figma.com/file/FILE_KEY/My-Design?type=design&node-id=12-34 \
  --impl http://localhost:3000/dashboard \
  --viewport 1440x900 \
  --threshold 0.95 \
  --output report.json
```

**Common flags:**

* `--ref <resource>`
* `--impl <resource>`
* `--ref-type <url|image|figma>` (optional)
* `--impl-type <url|image|figma>` (optional)
* `--viewport <WIDTHxHEIGHT>` (default: `1440x900`)
* `--threshold <float>` (default: `0.95`)
* `--metrics <comma-separated>`
  Example: `pixel,layout,typography,color,content`
  (default: all)
* `--ignore-selectors "<sel1,sel2>"` (for DOM-based comparisons)
* `--ignore-regions <path to JSON>` (manual bounding boxes to ignore)
* `--format json|pretty` (default: `json`)
* `--output <path>` (if omitted, prints to stdout)
* `--keep-artifacts` (keep intermediate screenshots, DOM JSON, diff maps)
* `--verbose`

#### 4.3.2 `dpc generate-code`

**Usage:**

```bash
dpc generate-code \
  --input https://www.figma.com/file/FILE_KEY/My-Design?type=design&node-id=12-34 \
  --stack html+tailwind \
  --viewport 1440x900 \
  --output hero.html
```

Flags:

* `--input <resource>` (URL / Figma / image – auto-detected)
* `--input-type <url|image|figma>` (optional override)
* `--stack <html+tailwind>` (MVP: only this stack; later React, etc.)
* `--viewport <WIDTHxHEIGHT>` (for URL/Figma; default `1440x900`)
* `--output <path>`

Internally calls a screenshot-to-code service (or equivalent LLM pipeline) and returns code only; errors are JSON if `--format json` is used. ([GitHub][2])

#### 4.3.3 `dpc quality` (experimental)

**Usage:**

```bash
dpc quality \
  --input http://localhost:3000 \
  --viewport 1440x900 \
  --output quality.json
```

Flags:

* `--input <resource>`
* `--input-type <url|image|figma>` (optional)
* `--viewport <WIDTHxHEIGHT>`
* `--output <path>`
* `--format json|pretty`

Quality mode returns a **heuristic design quality score** in [0,1] plus warnings (alignment, spacing, contrast…).

---

## 5. Normalized Internal Representation

Every input (ref/impl) is converted into a unified `NormalizedView`.

```ts
type NormalizedView = {
  kind: 'url' | 'image' | 'figma';

  screenshotPath: string; // normalized PNG on disk
  width: number;
  height: number;

  dom?: DomSnapshot;        // for URL kind
  figmaTree?: FigmaSnapshot; // for Figma kind
  ocrBlocks?: OcrBlock[];   // for pure-image or when requested
};
```

### 5.1 URL → NormalizedView

* Render with **headless Chromium via Playwright** at given viewport.
* Wait for **network idle** (configurable timeout).
* Capture:

  * Screenshot (PNG).
  * DOM snapshot:

    * Node tree (tag, attributes, text).
    * Bounding boxes in viewport coordinates.
    * Computed styles for key properties (font, color, etc.).

### 5.2 Figma → NormalizedView

* Use **Figma REST API** to:

  * Fetch file and frame JSON.
  * Export frame as PNG at given width/height (or Figma scale factor).
  * Extract:

    * Nodes with bounding boxes.
    * Text nodes with styles.
    * Fill colors, font family/weight/size. ([Figma Developer Docs][1])

### 5.3 Image → NormalizedView

* Load via Rust `image` crate.
* If ref/impl sizes differ:

  * Default: scale impl to ref resolution (letterbox if aspect ratio mismatch).
  * Option `--no-resize` to disable scaling.

### 5.4 OCR (optional)

* Use **Tesseract** via Rust binding or subprocess when:

  * No DOM or Figma tree exists, and
  * Content metrics are requested. ([GitHub][3])

---

## 6. Metrics & Algorithms

DPC computes a set of metric scores per compare run:

```ts
type MetricScores = {
  pixel?: PixelMetric;
  layout?: LayoutMetric;
  typography?: TypographyMetric;
  color?: ColorMetric;
  content?: ContentMetric;
};
```

Each metric provides:

* `score ∈ [0,1]` where **1 is best**.
* Structured diff information (regions, elements, explanations).

### 6.1 Pixel / Perceptual Similarity

**Goal:** detect raw visual differences in a human-aligned way.

Implementation:

* Use a Rust crate like `image-compare` (SSIM/RMS) or a Rust port of **dssim** (multi-scale SSIM) to approximate human visual similarity. ([Crates][4])
* Optionally expose strict per-pixel diff for debugging.

Process:

1. Convert both screenshots to common color space.
2. Compute SSIM/dSSIM or similar perceptual metric.
3. Normalize to [0,1] where 1 = identical.
4. Generate a **diff heatmap** and cluster “hot” pixels into regions.

Output model:

```ts
type PixelMetric = {
  score: number;
  diffRegions: Array<{
    x: number; y: number; w: number; h: number;
    severity: 'minor' | 'moderate' | 'major';
    reason: 'pixel_change' | 'anti_aliasing' | 'rendering_noise';
  }>;
};
```

Optional future enhancement:

* Layer in **LPIPS** (deep-feature-based perceptual metric) as an additional channel, using the open-source implementation. ([GitHub][5])

### 6.2 Layout / Structure Similarity

**Goal:** detect structural differences even when pixels are close (or vice versa).

Inspired by layout similarity work like **Rico** and **LayoutGMN**, which represent UIs as graph structures and measure structural similarity. ([Kaggle][6])

Algorithm (MVP, heuristic, no training):

1. **Element extraction**:

   * From DOM or Figma:

     * Identify elements: buttons/links, headings, text blocks, images, inputs, etc.
     * Standardize: `type`, `boundingBox`, `textLabel`.

2. **Normalize coordinates**:

   * Represent boxes as `(x, y, w, h)` in [0,1] relative to viewport.

3. **Graph construction**:

   * Nodes = elements.
   * Edges = basic spatial relations:

     * Parent-child.
     * Above/below.
     * Left/right (ordering by center coordinates).

4. **Element matching**:

   * Greedy matching between ref and impl nodes using:

     * Type match.
     * Text label similarity.
     * Proximity of normalized center + size.
   * Compute:

     * Match rate of ref elements.
     * Average IoU of matched boxes.

5. **Score**:

   * `layout_score = 0.5 * match_rate + 0.5 * avg_iou`.
   * Penalize extra unmatched elements in impl.

Output:

```ts
type LayoutMetric = {
  score: number;
  diffRegions: Array<{
    x: number; y: number; w: number; h: number;
    kind: 'missing_element' | 'extra_element' |
          'position_shift' | 'size_change';
    elementType?: string;
    label?: string;
  }>;
};
```

Future: we can swap the heuristic for a learned graph-matching model (LayoutGMN-style) without changing the external schema.

### 6.3 Typography Similarity

**Goal:** catch font-family, weight, size, and line-height differences.

Inputs:

* DOM computed style or Figma text styles.

Algorithm:

1. For matched elements (layout metric), extract typography:

   * `font-family`, `font-size`, `font-weight`, `line-height`.
2. Define equivalence:

   * Map families to canonical groups (e.g. `-apple-system`, `BlinkMacSystemFont`, `system-ui` → `system-sans`).
   * Map font-weight numeric values to categories (light/regular/medium/bold).
3. Compute penalties per element:

   * Font family mismatch → high penalty.
   * Size ratio difference beyond tolerance (e.g. > 10%) → medium penalty.
   * Weight category mismatch → medium penalty.
4. `typography_score = 1 - clamp(weighted_avg_penalty, 0, 1)`.

Output:

```ts
type TypographyMetric = {
  score: number;
  diffs: Array<{
    elementIdRef?: string;
    elementIdImpl?: string;
    issues: Array<
      'font_family_mismatch' |
      'font_size_diff' |
      'font_weight_diff' |
      'line_height_diff'
    >;
    details?: Record<string, any>;
  }>;
};
```

### 6.4 Color Palette Similarity

**Goal:** ensure theme/colors match.

Algorithm:

1. Extract dominant colors from each screenshot (e.g. k-means clustering in Lab color space).
2. Compare:

   * Pairwise distances between major colors.
   * Overlap of palette coverage by area (approximate fraction of image each color cluster covers).
3. Optionally cross-check Figma color tokens vs CSS custom properties in DOM.

Output:

```ts
type ColorMetric = {
  score: number;
  diffs: Array<{
    kind: 'primary_color_shift' | 'accent_color_shift' | 'background_color_shift';
    refColor: string;
    implColor: string;
    deltaE?: number;
  }>;
};
```

### 6.5 Content Similarity

**Goal:** ensure the same text content appears where it should.

Inputs:

* DOM text nodes, Figma text nodes, or OCR text blocks.

Algorithm:

1. Extract text strings:

   * Normalize whitespace, case, punctuation.
2. Match strings (e.g. with fuzzy matching for headings/labels).
3. Compute:

   * Fraction of ref strings matched in impl.
   * Penalty for extra “big” strings (e.g. large headings that don’t appear in ref).
4. `content_score = f(match_rate, extra_penalty)`.

Output:

```ts
type ContentMetric = {
  score: number;
  missingText: string[];
  extraText: string[];
};
```

---

## 7. Combined Score & Thresholds

Top-level similarity:

```text
similarity =
  w_pixel     * pixel_score     +
  w_layout    * layout_score    +
  w_typography* typography_score+
  w_color     * color_score     +
  w_content   * content_score
```

Default weights (configurable):

* `w_pixel`      = 0.35
* `w_layout`     = 0.25
* `w_typography` = 0.15
* `w_color`      = 0.15
* `w_content`    = 0.10

MVP behavior:

* If a metric cannot be computed (e.g. no content data), redistribute its weight proportionally across remaining metrics or treat as `null` and explicitly mark missing.

Threshold semantics:

* Default `--threshold 0.95`:

  * `similarity >= threshold` → `passed: true`.
  * Otherwise `passed: false`.

---

## 8. Output JSON Schema (MVP Shape)

Example for `dpc compare`:

```jsonc
{
  "version": "0.2.0",
  "mode": "compare",
  "ref": { "kind": "figma", "value": "https://www.figma.com/file/...node-id=..." },
  "impl": { "kind": "url", "value": "http://localhost:3000/dashboard" },
  "viewport": { "width": 1440, "height": 900 },

  "similarity": 0.94,
  "threshold": 0.95,
  "passed": false,

  "metrics": {
    "pixel": {
      "score": 0.98,
      "diffRegions": [
        { "x": 820, "y": 410, "w": 120, "h": 40,
          "severity": "minor", "reason": "pixel_change" }
      ]
    },
    "layout": {
      "score": 0.91,
      "diffRegions": [
        { "x": 120, "y": 280, "w": 200, "h": 50,
          "kind": "position_shift", "elementType": "button",
          "label": "Get started" }
      ]
    },
    "typography": {
      "score": 0.87,
      "diffs": [
        {
          "elementIdRef": "ref:h1#main-title",
          "elementIdImpl": "impl:h1#main-title",
          "issues": ["font_weight_diff", "font_size_diff"],
          "details": { "refSize": 32, "implSize": 28 }
        }
      ]
    },
    "color": {
      "score": 0.99,
      "diffs": []
    },
    "content": {
      "score": 0.96,
      "missingText": [],
      "extraText": []
    }
  },

  "summary": {
    "topIssues": [
      "Main CTA button is shifted down by ~8px vs spec.",
      "Main header is lighter and smaller than Figma design."
    ]
  }
}
```

---

## 9. Reference-Free Quality Mode (Experimental)

`dpc quality` returns:

```jsonc
{
  "version": "0.2.0",
  "mode": "quality",
  "input": { "kind": "url", "value": "http://localhost:3000" },
  "viewport": { "width": 1440, "height": 900 },
  "score": 0.78,
  "findings": [
    {
      "severity": "warning",
      "type": "alignment_inconsistent",
      "message": "Primary buttons in hero section are not aligned on the same left edge."
    },
    {
      "severity": "info",
      "type": "spacing_inconsistent",
      "message": "Detected 5+ distinct vertical spacing values (8, 11, 13, 19, 23px)."
    }
  ]
}
```

Initial heuristics:

* **Alignment**: cluster x-positions of left edges; flag elements that deviate heavily.
* **Spacing**: look at gaps between neighboring elements, cluster values, flag outliers.
* **Contrast**: approximate text/background contrast, flag low contrast.
* **Hierarchy**: check for at least 2–3 distinct text size tiers (title / subtitle / body).

This mode is explicitly labeled **experimental** and not used to gate CI by default.

---

## 10. Implementation Notes & Tech Choices

### 10.1 Language & Platform

* **Language:** Rust (binary `dpc`)

  * Image processing: `image` + `image-compare` + possibly `dssim`. ([Crates][4])
  * Good story for calling C libraries (Tesseract) and external tools.
  * Easy to package as static binary / Docker image.

* **Rendering:** Playwright or similar for headless Chromium.

* **External tools / services:**

  * Tesseract OCR. ([GitHub][3])
  * screenshot-to-code (OSS) or similar as a separate service for codegen. ([GitHub][2])

### 10.2 Performance Targets (Soft)

* Single `dpc compare` (URL↔URL, medium-complex SPA, 1440x900):

  * Target: a few seconds end-to-end in CI environment.
* Keep pipeline modular so metrics can be toggled for performance (e.g. disable OCR when unnecessary).

---

## 11. References: Libraries, Tools & Methodologies

This section is for engineers to dig deeper into algorithms and existing building blocks.

### 11.1 Pixel / Perceptual Image Comparison

* **pixelmatch** (JS) – small, fast pixel-level image diff with anti-aliasing detection and perceptual color diff; widely used in visual regression tools. ([GitHub][7])
* **Resemble.js** (JS) – image comparison library with diff image generation and options to ignore colors/antialiasing; used in many testing stacks. ([rsmbl.github.io][8])
* **image-compare** (Rust) – crate providing SSIM and RMS comparison for grayscale/RGB images; suitable for screenshot comparisons. ([Crates][4])
* **dssim** (Rust/C) – “RGBA Structural Similarity” tool implementing multi-scale SSIM in Rust, approximating human vision; good for perceptual difference measurement. ([GitHub][9])
* **LPIPS (Learned Perceptual Image Patch Similarity)** – deep-feature-based metric that correlates well with human perceptual judgments; can be added later for more advanced perceptual scoring. ([GitHub][5])

### 11.2 Layout & UI Structure Similarity

* **Rico dataset** – large-scale Android app UI dataset (~66k+ UI screens, ~3M UI elements) exposing visual, textual, structural, and interactive design properties. Good reference for how to represent UIs as structured graphs. ([Kaggle][6])
* **LayoutGMN** – “LayoutGMN: Neural Graph Matching for Structural Layout Similarity”, CVPR 2021; uses graph matching networks to learn layout similarity over 2D layouts (including UIs). Contains a PyTorch implementation and graph-construction logic that can inspire our heuristic layout graph. ([arXiv][10])

### 11.3 Embedding-Based / Deep-Feature Similarity

* **LPIPS paper & code** – demonstrates the “unreasonable effectiveness” of deep features for perceptual similarity and provides a reference implementation for computing patch-level distances. ([GitHub][5])
* **CLIP** – OpenAI’s Contrastive Language–Image Pretraining model; image encoder can be used to derive high-level visual embeddings for similarity beyond pure pixels. ([openai.com][11])
* **DINOv2** – self-supervised visual features from Meta; produces robust general-purpose visual embeddings suitable for similarity and retrieval tasks. ([arXiv][12])

### 11.4 Figma & Design Integration

* **Figma REST API** – official API for accessing files, images, and components; used to fetch frame PNGs and node trees for layout/typography analysis. ([Figma Developer Docs][1])
* Figma’s recent **Dev Mode MCP server** (Model Context Protocol) is an emerging way for AI tools to directly access Figma design data (colors, layout, etc.) and could be a future integration target for DPC. ([theverge.com][13])

### 11.5 Screenshot-to-Code / UI2Code

* **screenshot-to-code** – open-source tool that converts screenshots, mockups, and Figma designs into HTML/Tailwind, React/Tailwind, etc., using vision + code LLMs. Ideal to wrap for `dpc generate-code`. ([GitHub][2])
* **UI2Code^N** – recent (2025) visual language model for UI-to-code generation, editing, and polishing in multi-turn workflows; relevant as a future engine for advanced codegen + polishing loops. ([arXiv][14])

### 11.6 OCR

* **Tesseract** – open-source OCR engine with neural network–based recognition (LSTM) and support for >100 languages; we use it via CLI or Rust bindings for extracting text from screenshots when DOM/Figma data is unavailable. ([GitHub][3])


[1]: https://developers.figma.com/docs/rest-api/?utm_source=chatgpt.com "Introduction | Developer Docs"
[2]: https://github.com/abi/screenshot-to-code?utm_source=chatgpt.com "abi/screenshot-to-code"
[3]: https://github.com/tesseract-ocr/tesseract?utm_source=chatgpt.com "Tesseract Open Source OCR Engine (main repository)"
[4]: https://crates.io/crates/image-compare?utm_source=chatgpt.com "image-compare - crates.io: Rust Package Registry"
[5]: https://github.com/richzhang/PerceptualSimilarity?utm_source=chatgpt.com "richzhang/PerceptualSimilarity: LPIPS metric. pip install lpips"
[6]: https://www.kaggle.com/datasets/onurgunes1993/rico-dataset?utm_source=chatgpt.com "RICO dataset"
[7]: https://github.com/mapbox/pixelmatch?utm_source=chatgpt.com "mapbox/pixelmatch: The smallest, simplest and fastest ..."
[8]: https://rsmbl.github.io/Resemble.js/?utm_source=chatgpt.com "Resemble.js : Image analysis - GitHub Pages"
[9]: https://github.com/kornelski/dssim?utm_source=chatgpt.com "kornelski/dssim: Image similarity comparison simulating ..."
[10]: https://arxiv.org/abs/2012.06547?utm_source=chatgpt.com "LayoutGMN: Neural Graph Matching for Structural Layout Similarity"
[11]: https://openai.com/index/clip/?utm_source=chatgpt.com "CLIP: Connecting text and images"
[12]: https://arxiv.org/abs/2304.07193?utm_source=chatgpt.com "[2304.07193] DINOv2: Learning Robust Visual Features ..."
[13]: https://www.theverge.com/news/679439/figma-dev-mode-mcp-server-beta-release?utm_source=chatgpt.com "Figma will let your AI access its design servers"
[14]: https://arxiv.org/abs/2511.08195?utm_source=chatgpt.com "UI2Code$^\text{N}$: A Visual Language Model for Test-Time Scalable Interactive UI-to-Code Generation"
