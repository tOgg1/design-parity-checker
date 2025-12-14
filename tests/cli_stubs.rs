use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dpc_lib::DpcOutput;

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

fn run_cmd(args: &[&str]) -> Output {
    Command::new(bin_path())
        .args(args)
        .output()
        .expect("run dpc command")
}

fn run_cmd_with_env(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(bin_path());
    cmd.args(args);
    for (key, val) in env {
        cmd.env(key, val);
    }
    cmd.output().expect("run dpc command")
}

fn parse_json(stdout: &[u8]) -> DpcOutput {
    serde_json::from_slice(stdout).expect("output should be valid JSON")
}

#[test]
fn generate_code_emits_html() {
    let output = run_cmd_with_env(
        &[
            "generate-code",
            "--input",
            asset("ref.png").to_str().unwrap(),
            "--stack",
            "html+tailwind",
            "--format",
            "json",
        ],
        &[("DPC_MOCK_CODE", "<section class=\"mock\">hello</section>")],
    );

    assert!(
        output.status.success(),
        "generate-code should exit 0, got {:?}",
        output.status.code()
    );

    match parse_json(&output.stdout) {
        DpcOutput::GenerateCode(out) => {
            assert_eq!(out.input.kind, dpc_lib::ResourceKind::Image);
            assert_eq!(
                out.code.as_deref(),
                Some("<section class=\"mock\">hello</section>")
            );
            let notes = out.summary.unwrap().top_issues;
            assert!(
                notes
                    .iter()
                    .any(|n| n.to_ascii_lowercase().contains("mock")),
                "summary should note mock usage"
            );
        }
        other => panic!("expected generate-code output, got {:?}", other),
    }
}

#[test]
fn quality_command_scores_with_findings() {
    let output = run_cmd(&[
        "quality",
        "--input",
        asset("ref.png").to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(
        output.status.success(),
        "quality command should exit 0, got {:?}",
        output.status.code()
    );

    match parse_json(&output.stdout) {
        DpcOutput::Quality(out) => {
            assert_eq!(out.input.kind, dpc_lib::ResourceKind::Image);
            assert!(
                (0.0..=1.0).contains(&out.score),
                "score should be normalized, got {}",
                out.score
            );
            assert!(
                !out.findings.is_empty(),
                "quality should emit heuristic findings"
            );
            assert!(
                out.findings.iter().any(|f| matches!(
                    f.finding_type,
                    dpc_lib::QualityFindingType::MissingHierarchy
                )),
                "quality findings should include a missing_hierarchy entry"
            );
        }
        other => panic!("expected quality output, got {:?}", other),
    }
}
