use std::path::PathBuf;
use std::process::ExitCode;

use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::types::ResourceKind;
use dpc_lib::{
    parse_resource, DpcError, DpcOutput, FindingSeverity, QualityFinding, QualityOutput,
    ResourceDescriptor, Viewport,
};

use crate::cli::OutputFormat;
use crate::formatting::{render_error, write_output};
use crate::settings::{flag_present, load_config};

/// Run the quality command.
#[allow(clippy::too_many_arguments)]
pub async fn run_quality(
    raw_args: &[String],
    config_path: Option<PathBuf>,
    verbose: bool,
    input: String,
    input_type: Option<crate::cli::ResourceType>,
    viewport: Viewport,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> ExitCode {
    let config = match load_config(config_path.as_deref()) {
        Ok(cfg) => cfg,
        Err(err) => return render_error(err, format, output.clone()),
    };
    let viewport = if flag_present(raw_args, "--viewport") {
        viewport
    } else {
        config.viewport
    };
    if verbose {
        eprintln!("Parsing input resource\u{2026}");
    }
    let input_res = match parse_resource(&input, input_type.map(resource_kind_from_cli)) {
        Ok(res) => res,
        Err(err) => {
            return render_error(DpcError::Config(err.to_string()), format, output.clone())
        }
    };
    if verbose {
        eprintln!(
            "Computed normalized input ({:?}); quality mode is currently stubbed",
            input_res.kind
        );
    }
    let body = DpcOutput::Quality(QualityOutput {
        version: DPC_OUTPUT_VERSION.to_string(),
        input: ResourceDescriptor {
            kind: input_res.kind,
            value: input_res.value,
        },
        viewport,
        score: 0.0,
        findings: vec![
            QualityFinding {
                severity: FindingSeverity::Info,
                finding_type: "not_implemented".to_string(),
                message: "Not implemented: quality scoring is coming soon; use `dpc compare` for parity checks and track findings manually.".to_string(),
            },
            QualityFinding {
                severity: FindingSeverity::Info,
                finding_type: "next_steps".to_string(),
                message: "Use mocks or artifacts to gather context: --keep-artifacts/--artifacts-dir retains screenshots/DOM for manual review.".to_string(),
            },
        ],
    });
    if let Err(err) = write_output(&body, format, output.clone()) {
        return render_error(DpcError::Config(err.to_string()), format, output);
    }
    ExitCode::SUCCESS
}

fn resource_kind_from_cli(rt: crate::cli::ResourceType) -> ResourceKind {
    match rt {
        crate::cli::ResourceType::Url => ResourceKind::Url,
        crate::cli::ResourceType::Image => ResourceKind::Image,
        crate::cli::ResourceType::Figma => ResourceKind::Figma,
    }
}
