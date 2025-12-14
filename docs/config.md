# Configuration (proposal)

> Note: Config file wiring may be in progress. This doc captures intended keys and defaults to unblock BXG/config work.

## File format
TOML, e.g. `dpc.toml`.

## Keys and defaults
```toml
[compare]
viewport = "1440x900"
threshold = 0.95
metrics = ["pixel", "layout", "typography", "color", "content"]
ignore_selectors = []

[weights]
pixel = 0.35
layout = 0.25
typography = 0.15
color = 0.15
content = 0.10

[timeouts]
navigation_ms = 30000        # Playwright navigate timeout
network_idle_ms = 10000      # networkidle wait
process_ms = 45000           # overall browser render budget

[artifacts]
keep = false
directory = ""               # optional; if set implies keep
```

## Notes
- `viewport` should parse as WIDTHxHEIGHT.
- `metrics` is optional; when omitted defaults apply. When both inputs lack DOM/figma, layout/typography/content are skipped.
- `ignore_selectors` is a comma/array list of CSS selectors for DOM-only ignores.
- `ignore_regions` remains a CLI flag (JSON file) for masking pixel/color; not proposed in TOML yet.
- `weights` renormalize over present metrics when some are absent.
- `timeouts` apply to URL/Playwright renders; `process` is the outer Playwright budget. Figma uses API timeouts internally.
- `artifacts.keep`/`directory` mirror `--keep-artifacts`/`--artifacts-dir`.

## CLI interplay (proposed)
- `--config path` opt-in: load TOML, then CLI flags override.
- If `directory` is set, treat as keep=true.
- Invalid keys/values should yield a config error (exit 2) with clear remediation.

## Example minimal
```toml
[compare]
viewport = "1280x720"
threshold = 0.9

[weights]
pixel = 0.4
layout = 0.3
typography = 0.1
color = 0.1
content = 0.1
```
