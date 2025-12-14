use dpc_lib::DpcOutput;
use image::RgbaImage;
use std::env;
use std::process::Command;
use tempfile::TempDir;

fn write_image(path: &std::path::Path, color: [u8; 4]) {
    let img = RgbaImage::from_pixel(4, 4, image::Rgba(color));
    img.save(path).expect("write image");
}

#[test]
fn compare_exit_code_passes_for_matching_images() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [10, 20, 30, 255]);
    write_image(&impl_path, [10, 20, 30, 255]);

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn compare_accepts_config_flag_and_still_passes() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    let cfg_path = dir.path().join("dpc.toml");
    write_image(&ref_path, [1, 2, 3, 255]);
    write_image(&impl_path, [1, 2, 3, 255]);
    std::fs::write(&cfg_path, "threshold = 0.9\n").expect("write config");

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--config",
            cfg_path.to_str().unwrap(),
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn compare_exit_code_fails_threshold_for_different_images() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [0, 0, 0, 255]);
    write_image(&impl_path, [255, 255, 255, 255]);

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(1));
}

#[test]
fn compare_exit_code_fails_threshold_for_different_images_pretty() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [0, 0, 0, 255]);
    write_image(&impl_path, [255, 255, 255, 255]);

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "pretty",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(1));
}

#[test]
fn compare_exit_code_returns_fatal_for_invalid_input() {
    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            "missing.png",
            "--impl",
            "also-missing.png",
            "--format",
            "json",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(2));
}

fn run_compare_pretty(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args(args)
        .output()
        .expect("run dpc compare")
}

fn parse_pretty(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("pretty output should be JSON")
}

#[test]
fn compare_pretty_exits_zero_for_matching_images() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [10, 20, 30, 255]);
    write_image(&impl_path, [10, 20, 30, 255]);

    let output = run_compare_pretty(&[
        "compare",
        "--ref",
        ref_path.to_str().unwrap(),
        "--impl",
        impl_path.to_str().unwrap(),
        "--format",
        "pretty",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Artifacts directory:"),
        "stderr should surface artifact directory on success"
    );
    let pretty = parse_pretty(&output.stdout);
    assert_eq!(pretty.get("mode").and_then(|v| v.as_str()), Some("compare"));
    assert_eq!(
        pretty.get("passed").and_then(|v| v.as_bool()),
        Some(true),
        "pretty output should show pass status, got {pretty}"
    );
}

#[test]
fn compare_pretty_exits_one_when_below_threshold() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [0, 0, 0, 255]);
    write_image(&impl_path, [255, 255, 255, 255]);

    let output = run_compare_pretty(&[
        "compare",
        "--ref",
        ref_path.to_str().unwrap(),
        "--impl",
        impl_path.to_str().unwrap(),
        "--format",
        "pretty",
        "--threshold",
        "0.99",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let pretty = parse_pretty(&output.stdout);
    assert_eq!(pretty.get("mode").and_then(|v| v.as_str()), Some("compare"));
    assert_eq!(
        pretty.get("passed").and_then(|v| v.as_bool()),
        Some(false),
        "pretty output should show fail status, got {pretty}"
    );
}

#[test]
fn compare_pretty_exits_two_for_missing_inputs() {
    let output = run_compare_pretty(&[
        "compare",
        "--ref",
        "missing.png",
        "--impl",
        "missing2.png",
        "--format",
        "pretty",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty for pretty errors"
    );
    let err = parse_pretty(&output.stdout);
    assert_eq!(err.get("mode").and_then(|v| v.as_str()), Some("error"));
    let msg = err
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        msg.contains("missing"),
        "stderr should mention missing inputs, got {msg}"
    );
}

#[test]
fn compare_pretty_writes_plain_when_output_path_set() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    let out_path = dir.path().join("out.txt");
    write_image(&ref_path, [10, 20, 30, 255]);
    write_image(&impl_path, [10, 20, 30, 255]);

    let output = run_compare_pretty(&[
        "compare",
        "--ref",
        ref_path.to_str().unwrap(),
        "--impl",
        impl_path.to_str().unwrap(),
        "--format",
        "pretty",
        "--output",
        out_path.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty(),
        "when writing to file, stdout should stay empty"
    );
    let content = std::fs::read_to_string(&out_path).expect("read pretty output");
    let json: serde_json::Value =
        serde_json::from_str(&content).expect("pretty output file should be JSON");
    assert_eq!(json.get("mode").and_then(|v| v.as_str()), Some("compare"));
    assert_eq!(json.get("passed").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn compare_figma_without_token_reports_config_error_and_remediation() {
    // Ensure FIGMA_TOKEN is unset for this test.
    let prev = env::var("FIGMA_TOKEN").ok();
    env::remove_var("FIGMA_TOKEN");

    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            "https://www.figma.com/file/FILE123/Mock?node-id=1-2",
            "--impl",
            "https://www.figma.com/file/FILE123/Mock?node-id=1-2",
            "--format",
            "json",
        ])
        .output()
        .expect("run dpc compare");

    if let Some(val) = prev {
        env::set_var("FIGMA_TOKEN", val);
    } else {
        env::remove_var("FIGMA_TOKEN");
    }

    assert_eq!(output.status.code(), Some(2));
    let err: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout JSON for figma error");
    assert_eq!(err.get("mode").and_then(|v| v.as_str()), Some("error"));
    let remediation = err
        .get("error")
        .and_then(|e| e.get("remediation"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    assert!(
        remediation.contains("FIGMA_TOKEN"),
        "expected FIGMA_TOKEN remediation, got: {remediation}"
    );
}

#[test]
fn compare_figma_without_node_id_reports_config_error_and_hint() {
    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            "https://www.figma.com/file/FILE123/MockWithoutNode",
            "--impl",
            "https://www.figma.com/file/FILE123/MockWithoutNode",
            "--format",
            "json",
        ])
        .output()
        .expect("run dpc compare");

    assert_eq!(output.status.code(), Some(2));
    let err: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout JSON for figma node-id error");
    let remediation = err
        .get("error")
        .and_then(|e| e.get("remediation"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        remediation.contains("node-id"),
        "expected node-id remediation, got: {remediation}"
    );
}

#[test]
fn generate_code_stub_exits_zero() {
    let dir = TempDir::new().expect("tempdir");
    let input_path = dir.path().join("input.png");
    write_image(&input_path, [128, 64, 32, 255]);

    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "generate-code",
            "--input",
            input_path.to_str().unwrap(),
            "--stack",
            "html+tailwind",
            "--viewport",
            "800x600",
            "--format",
            "json",
        ])
        .output()
        .expect("run dpc generate-code");

    assert_eq!(output.status.code(), Some(0));
    let body: DpcOutput =
        serde_json::from_slice(&output.stdout).expect("generate-code output should be JSON");
    match body {
        DpcOutput::GenerateCode(out) => {
            assert_eq!(out.input.value, input_path.to_string_lossy());
            let summary = out
                .summary
                .and_then(|s| s.top_issues.first().cloned())
                .unwrap_or_default();
            assert!(
                summary.to_ascii_lowercase().contains("not implemented"),
                "expected not-implemented summary, got: {summary}"
            );
        }
        other => panic!("expected generate-code output, got {:?}", other),
    }
}

#[test]
fn generate_code_pretty_outputs_stub_message() {
    let dir = TempDir::new().expect("tempdir");
    let input_path = dir.path().join("input.png");
    write_image(&input_path, [128, 64, 32, 255]);

    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "generate-code",
            "--input",
            input_path.to_str().unwrap(),
            "--stack",
            "html+tailwind",
            "--viewport",
            "800x600",
            "--format",
            "pretty",
        ])
        .output()
        .expect("run dpc generate-code");

    assert_eq!(output.status.code(), Some(0));
    let body: DpcOutput =
        serde_json::from_slice(&output.stdout).expect("pretty output should stay JSON when piped");
    match body {
        DpcOutput::GenerateCode(out) => {
            let summary = out
                .summary
                .and_then(|s| s.top_issues.first().cloned())
                .unwrap_or_default();
            assert!(
                summary.to_ascii_lowercase().contains("not implemented"),
                "expected not-implemented summary, got: {summary}"
            );
        }
        other => panic!("expected generate-code output, got {:?}", other),
    }
}

#[test]
fn quality_stub_exits_zero() {
    let dir = TempDir::new().expect("tempdir");
    let input_path = dir.path().join("input.png");
    write_image(&input_path, [200, 200, 200, 255]);

    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "quality",
            "--input",
            input_path.to_str().unwrap(),
            "--viewport",
            "640x480",
            "--format",
            "json",
        ])
        .output()
        .expect("run dpc quality");

    assert_eq!(output.status.code(), Some(0));
    let body: DpcOutput =
        serde_json::from_slice(&output.stdout).expect("quality output should be JSON");
    match body {
        DpcOutput::Quality(out) => {
            assert_eq!(out.input.value, input_path.to_string_lossy());
            assert_eq!(out.viewport.width, 640);
            assert_eq!(out.viewport.height, 480);
            let first = out
                .findings
                .get(0)
                .map(|f| f.message.clone())
                .unwrap_or_default();
            assert!(
                first.to_ascii_lowercase().contains("not implemented"),
                "expected not-implemented finding, got: {first}"
            );
        }
        other => panic!("expected quality output, got {:?}", other),
    }
}

#[test]
fn quality_pretty_outputs_stub_message() {
    let dir = TempDir::new().expect("tempdir");
    let input_path = dir.path().join("input.png");
    write_image(&input_path, [200, 200, 200, 255]);

    let output = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "quality",
            "--input",
            input_path.to_str().unwrap(),
            "--viewport",
            "640x480",
            "--format",
            "pretty",
        ])
        .output()
        .expect("run dpc quality");

    assert_eq!(output.status.code(), Some(0));
    let body: DpcOutput =
        serde_json::from_slice(&output.stdout).expect("pretty output should stay JSON when piped");
    match body {
        DpcOutput::Quality(out) => {
            let first = out
                .findings
                .get(0)
                .map(|f| f.message.clone())
                .unwrap_or_default();
            assert!(
                first.to_ascii_lowercase().contains("not implemented"),
                "expected not-implemented finding, got: {first}"
            );
        }
        other => panic!("expected quality output, got {:?}", other),
    }
}

#[test]
fn compare_pretty_exit_code_passes_for_matching_images() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [10, 20, 30, 255]);
    write_image(&impl_path, [10, 20, 30, 255]);

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "pretty",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn compare_pretty_exit_code_fails_threshold_for_different_images() {
    let dir = TempDir::new().expect("tempdir");
    let ref_path = dir.path().join("ref.png");
    let impl_path = dir.path().join("impl.png");
    write_image(&ref_path, [0, 0, 0, 255]);
    write_image(&impl_path, [255, 255, 255, 255]);

    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            ref_path.to_str().unwrap(),
            "--impl",
            impl_path.to_str().unwrap(),
            "--format",
            "pretty",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(1));
}

#[test]
fn compare_pretty_exit_code_returns_fatal_for_invalid_input() {
    let status = Command::new(env!("CARGO_BIN_EXE_dpc"))
        .args([
            "compare",
            "--ref",
            "missing.png",
            "--impl",
            "also-missing.png",
            "--format",
            "pretty",
        ])
        .status()
        .expect("run dpc");
    assert_eq!(status.code(), Some(2));
}
