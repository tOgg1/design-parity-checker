# Design Parity Checker – Output Contract (v0.2.0)

This document describes the CLI output contract for all commands. The schema is shared across `json` and `pretty` formats; pretty is simply pretty‑printed JSON.

## Common envelope

- `mode`: `"compare" | "generate-code" | "quality" | "error"`
- `version`: schema version (`DPC_OUTPUT_VERSION`, currently `0.2.0`)

## Compare success payload

```json
{
  "mode": "compare",
  "version": "0.2.0",
  "ref": {"kind": "url", "value": "https://ref.example"},
  "impl": {"kind": "image", "value": "impl.png"},
  "viewport": {"width": 1440, "height": 900},
  "similarity": 0.97,
  "threshold": 0.95,
  "passed": true,
  "metrics": {
    "pixel": {"score": 0.98, "diffRegions": []},
    "layout": null,
    "typography": null,
    "color": {"score": 0.96, "diffs": []},
    "content": null
  },
  "summary": {
    "topIssues": [
      "Design parity check passed (97.0% similarity, threshold: 95.0%)"
    ]
  },
  "artifacts": {
    "directory": "/tmp/dpc-1234-1700000000000",
    "kept": true,
    "refScreenshot": "/tmp/dpc-1234-1700000000000/ref_screenshot.png",
    "implScreenshot": "/tmp/dpc-1234-1700000000000/impl_screenshot.png",
    "diffImage": null,
    "refDomSnapshot": "/tmp/dpc-1234-1700000000000/ref_dom.json",
    "implDomSnapshot": "/tmp/dpc-1234-1700000000000/impl_dom.json",
    "refFigmaSnapshot": null,
    "implFigmaSnapshot": null
  }
}
```

Notes:
- `artifacts` is present only when `--keep-artifacts` or `--artifacts-dir` is supplied. Paths are absolute. `kept` indicates whether the artifacts directory will persist after command exit.
- `metrics` fields are optional and omitted when not computed.

## Error payload

```json
{
  "mode": "error",
  "version": "0.2.0",
  "error": {
    "category": "config",
    "message": "File not found: missing.png",
    "remediation": "Check file paths/permissions."
  }
}
```

Behavior:
- JSON mode writes errors to stdout; pretty mode writes pretty JSON to stdout (or to `--output` if specified).
- Exit codes: `0` success, `1` threshold failure (compare), `2` errors (config/network/runtime).

## GenerateCode payload (stub)

```json
{
  "mode": "generate-code",
  "version": "0.2.0",
  "input": {"kind": "figma", "value": "https://www.figma.com/file/…"},
  "viewport": {"width": 1440, "height": 900},
  "stack": "html+tailwind",
  "outputPath": "output.html",
  "code": null,
  "summary": {"topIssues": ["generate-code is not implemented yet"]}
}
```

## Quality payload (stub)

```json
{
  "mode": "quality",
  "version": "0.2.0",
  "input": {"kind": "url", "value": "https://example.com"},
  "viewport": {"width": 1440, "height": 900},
  "score": 0.0,
  "findings": [
    {
      "severity": "info",
      "type": "not_implemented",
      "message": "quality mode not implemented yet"
    }
  ]
}
```
