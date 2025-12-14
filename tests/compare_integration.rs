use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dpc_lib::error::ErrorCategory;
use dpc_lib::{DpcOutput, ResourceKind};
use image::{ImageBuffer, Rgba};
use serde_json::Value;
use tempfile::tempdir;

fn bin_path() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_dpc")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("debug")
                .join(if cfg!(windows) { "dpc.exe" } else { "dpc" })
        })
}

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_assets")
        .join(name)
}

fn run_compare(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(bin_path());
    cmd.args(args);

    for (k, v) in envs {
        cmd.env(k, v);
    }

    cmd.output().expect("run compare command")
}

fn parse_output(stdout: &[u8]) -> DpcOutput {
    serde_json::from_slice(stdout).expect("compare output should be valid JSON")
}

fn parse_error(stdout: &[u8]) -> DpcOutput {
    serde_json::from_slice(stdout).expect("error payload should be valid JSON")
}

fn parse_pretty(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("pretty output should be JSON")
}

fn parse_error_pretty(stderr: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stderr).expect("pretty error output should be JSON")
}

#[test]
fn compare_emits_artifacts_when_keep_artifacts_is_enabled() {
    let dir = tempdir().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    let artifacts_dir = dir.path().join("artifacts");

    let ref_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    let impl_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    ref_img.save(&ref_path).unwrap();
    impl_img.save(&impl_path).unwrap();

    let output = run_compare(
        &[
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.9",
            "--keep-artifacts",
            "--artifacts-dir",
            artifacts_dir.to_str().unwrap(),
        ],
        &[],
    );

    assert_eq!(output.status.code(), Some(0));

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            let artifacts = out
                .artifacts
                .expect("artifacts block should be present when --keep-artifacts is set");
            assert!(
                artifacts.kept,
                "kept should be true when keep-artifacts is set"
            );
            assert_eq!(
                artifacts.directory, artifacts_dir,
                "artifacts directory should echo the requested path"
            );

            let ref_shot = artifacts.ref_screenshot.expect("ref screenshot path");
            assert!(ref_shot.exists(), "ref screenshot should exist on disk");
            let impl_shot = artifacts.impl_screenshot.expect("impl screenshot path");
            assert!(impl_shot.exists(), "impl screenshot should exist on disk");
            let diff_heatmap = artifacts.diff_image.expect("diff heatmap path");
            assert!(diff_heatmap.exists(), "diff heatmap should exist on disk");
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn compare_reports_artifacts_even_when_not_kept() {
    let dir = tempdir().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");

    let ref_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    let impl_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    ref_img.save(&ref_path).unwrap();
    impl_img.save(&impl_path).unwrap();

    let output = run_compare(
        &[
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.9",
        ],
        &[],
    );

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Artifacts directory"),
        "stderr should include artifact directory path"
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            let artifacts = out
                .artifacts
                .expect("artifacts block should be present even when not kept");
            assert!(
                !artifacts.kept,
                "kept should be false when --keep-artifacts is not set"
            );
            assert!(
                !artifacts.directory.as_os_str().is_empty(),
                "artifacts directory should be populated"
            );
            assert!(
                artifacts.ref_screenshot.is_some(),
                "ref screenshot path should be present"
            );
            assert!(
                artifacts.impl_screenshot.is_some(),
                "impl screenshot path should be present"
            );
            assert!(
                artifacts.diff_image.is_none(),
                "diff heatmap should not be generated when artifacts are not kept"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn ignore_regions_masks_pixel_differences() {
    let dir = tempdir().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    let ignore_path = dir.path().join("ignore.json");

    let ref_img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_pixel(4, 4, Rgba([255, 255, 255, 255]));
    let impl_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
    ref_img.save(&ref_path).unwrap();
    impl_img.save(&impl_path).unwrap();
    std::fs::write(&ignore_path, r#"[{"x":0.0,"y":0.0,"w":1.0,"h":1.0}]"#).unwrap();

    let output = run_compare(
        &[
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.99",
            "--ignore-regions",
            ignore_path.to_str().unwrap(),
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "ignore-regions should mask diffs and allow compare to pass"
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(out.passed, "expected masked diff to meet the threshold");
            let pixel = out.metrics.pixel.as_ref().expect("pixel metric present");
            assert!(
                pixel.score >= 0.99,
                "pixel score should be near-perfect after masking, got {}",
                pixel.score
            );
            assert!(
                pixel.diff_regions.is_empty(),
                "pixel diffs should be empty after masking the viewport"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn ignore_regions_accepts_width_height_keys_and_normalized_values() {
    let dir = tempdir().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");

    let ref_img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(4, 4, Rgba([10, 10, 10, 255]));
    let impl_img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_pixel(4, 4, Rgba([250, 250, 250, 255]));
    ref_img.save(&ref_path).unwrap();
    impl_img.save(&impl_path).unwrap();

    let output = run_compare(
        &[
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.99",
            "--ignore-regions",
            asset("ignore_regions_example.json").to_str().unwrap(),
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "normalized width/height keys should mask the full image"
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(out.passed, "expected masked diff to meet the threshold");
            let pixel = out.metrics.pixel.as_ref().expect("pixel metric present");
            assert!(
                pixel.diff_regions.is_empty(),
                "masked comparison should not report pixel diffs"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn image_inputs_emit_json_and_pass() {
    let output = run_compare(
        &[
            "compare",
            "--ref",
            asset("ref.png").to_str().unwrap(),
            "--impl",
            asset("impl_identical.png").to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.90",
        ],
        &[],
    );

    assert!(
        output.status.success(),
        "expected success exit, got {:?}",
        output.status.code()
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(out.passed, "expected passed=true");
            assert!(
                out.similarity >= out.threshold,
                "similarity {} should meet/exceed threshold {}",
                out.similarity,
                out.threshold
            );
            assert!(
                out.summary
                    .as_ref()
                    .map_or(false, |s| !s.top_issues.is_empty()),
                "summary should include top issues"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn image_inputs_fail_when_below_threshold() {
    let output = run_compare(
        &[
            "compare",
            "--ref",
            asset("ref.png").to_str().unwrap(),
            "--impl",
            asset("impl_different.png").to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "0.99",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit with failure code"
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(
                !out.passed,
                "expected passed=false when similarity below threshold"
            );
            assert!(
                out.similarity < out.threshold,
                "similarity {} should be below threshold {}",
                out.similarity,
                out.threshold
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn url_inputs_use_mock_renderer() {
    let output = run_compare(
        &[
            "compare",
            "--ref",
            "https://example.com/design",
            "--impl",
            "https://example.com/build",
            "--format",
            "json",
            "--threshold",
            "0.99",
        ],
        &[
            ("DPC_MOCK_RENDER_REF", asset("ref.png").to_str().unwrap()),
            (
                "DPC_MOCK_RENDER_IMPL",
                asset("impl_different.png").to_str().unwrap(),
            ),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(1),
        "URL mock run should fail threshold"
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(matches!(out.ref_resource.kind, ResourceKind::Url));
            assert!(matches!(out.impl_resource.kind, ResourceKind::Url));
            assert!(
                !out.passed,
                "expected failure when mock images differ significantly"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn figma_inputs_use_mock_renderer() {
    let output = run_compare(
        &[
            "compare",
            "--ref",
            "https://www.figma.com/file/FILE123/Mock?node-id=1-2",
            "--impl",
            "https://www.figma.com/file/FILE123/Mock?node-id=2-3",
            "--format",
            "json",
            "--threshold",
            "0.90",
        ],
        &[
            ("DPC_MOCK_RENDER_REF", asset("ref.png").to_str().unwrap()),
            (
                "DPC_MOCK_RENDER_IMPL",
                asset("impl_identical.png").to_str().unwrap(),
            ),
            ("FIGMA_TOKEN", "dummy-token"),
        ],
    );

    assert!(
        output.status.success(),
        "figma mock run should pass: {:?}",
        output.status.code()
    );

    match parse_output(&output.stdout) {
        DpcOutput::Compare(out) => {
            assert!(matches!(out.ref_resource.kind, ResourceKind::Figma));
            assert!(matches!(out.impl_resource.kind, ResourceKind::Figma));
            assert!(
                out.passed,
                "expected passing result for matching mock images"
            );
        }
        other => panic!("expected compare output, got {:?}", other),
    }
}

#[test]
fn invalid_input_exits_with_fatal_code() {
    let missing = asset("missing.png");
    let output = run_compare(
        &[
            "compare",
            "--ref",
            missing.to_str().unwrap(),
            "--impl",
            asset("ref.png").to_str().unwrap(),
            "--format",
            "json",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "fatal/config errors should exit with code 2"
    );

    match parse_error(&output.stdout) {
        DpcOutput::Error(err) => {
            assert_eq!(err.error.category, ErrorCategory::Config);
            let message = err.error.message.to_ascii_lowercase();
            assert!(
                message.contains("not found") || message.contains("missing"),
                "error message should describe missing input, got: {message}"
            );
        }
        other => panic!("expected error payload, got {:?}", other),
    };
}

#[test]
fn pretty_output_serializes_and_marks_status() {
    let output = run_compare(
        &[
            "compare",
            "--ref",
            asset("ref.png").to_str().unwrap(),
            "--impl",
            asset("impl_identical.png").to_str().unwrap(),
            "--format",
            "pretty",
            "--threshold",
            "0.90",
        ],
        &[],
    );

    assert!(
        output.status.success(),
        "expected success exit, got {:?}",
        output.status.code()
    );

    let pretty = parse_pretty(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty() || stderr.contains("Artifacts directory"),
        "unexpected stderr on success: {stderr}"
    );
    assert_eq!(pretty.get("mode").and_then(|v| v.as_str()), Some("compare"));
    assert_eq!(
        pretty.get("passed").and_then(|v| v.as_bool()),
        Some(true),
        "pretty output should indicate pass status, got {pretty}"
    );
}

#[test]
fn pretty_errors_use_exit_code_two_and_stderr() {
    let missing = asset("missing.png");
    let output = run_compare(
        &[
            "compare",
            "--ref",
            missing.to_str().unwrap(),
            "--impl",
            asset("ref.png").to_str().unwrap(),
            "--format",
            "pretty",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "fatal/config errors should exit with code 2"
    );
    assert!(
        output.stderr.is_empty(),
        "pretty errors should write JSON to stdout"
    );
    let err = parse_error_pretty(&output.stdout);
    assert_eq!(err.get("mode").and_then(|v| v.as_str()), Some("error"));
    let msg = err
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        msg.contains("missing"),
        "error message should mention missing file, got {msg}"
    );
}

#[test]
fn empty_ignore_regions_file_is_fatal_with_clear_message() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_path = dir.path().join("empty_regions.json");
    let mut f = std::fs::File::create(&empty_path).expect("create file");
    writeln!(f, "[]").expect("write file");

    let output = run_compare(
        &[
            "compare",
            "--ref",
            asset("ref.png").to_str().unwrap(),
            "--impl",
            asset("impl_identical.png").to_str().unwrap(),
            "--ignore-regions",
            empty_path.to_str().unwrap(),
            "--format",
            "json",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "empty ignore-regions should be fatal"
    );
    let err: Value = serde_json::from_slice(&output.stdout).expect("stdout JSON");
    assert_eq!(err["error"]["category"], "config");
    let msg = err["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        msg.contains("no regions"),
        "message should mention empty/zero regions: {msg}"
    );
}

#[test]
fn invalid_ignore_regions_file_exits_with_error() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let bad_path = dir.path().join("bad_regions.json");
    let mut f = std::fs::File::create(&bad_path).expect("create file");
    writeln!(f, "{{not valid json").expect("write file");

    let output = run_compare(
        &[
            "compare",
            "--ref",
            asset("ref.png").to_str().unwrap(),
            "--impl",
            asset("impl_identical.png").to_str().unwrap(),
            "--ignore-regions",
            bad_path.to_str().unwrap(),
            "--format",
            "json",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid ignore-regions should be fatal"
    );
    let err: Value = serde_json::from_slice(&output.stdout).expect("stdout JSON");
    assert_eq!(err["error"]["category"], "config");
    let message = err["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        message.contains("ignore-regions"),
        "expected message to mention ignore-regions, got: {message}"
    );
    assert!(
        message.contains("width"),
        "expected message to describe expected shape {{x,y,width,height}}, got: {message}"
    );
}
