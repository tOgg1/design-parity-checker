use std::path::PathBuf;
use std::process::ExitCode;

use dpc_lib::output::DPC_OUTPUT_VERSION;
use dpc_lib::types::ResourceKind;
use dpc_lib::{parse_resource, DpcError, DpcOutput, GenerateCodeOutput, ResourceDescriptor, Summary, Viewport};

use crate::cli::OutputFormat;
use crate::formatting::{render_error, write_output};
use crate::settings::{flag_present, load_config};

/// Run the generate-code command.
#[allow(clippy::too_many_arguments)]
pub async fn run_generate_code(
    raw_args: &[String],
    config_path: Option<PathBuf>,
    verbose: bool,
    input: String,
    input_type: Option<crate::cli::ResourceType>,
    viewport: Viewport,
    stack: String,
    output: Option<PathBuf>,
    format: OutputFormat,
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
    let viewport = Some(viewport);
    let input_res = match parse_resource(&input, input_type.map(resource_kind_from_cli)) {
        Ok(res) => res,
        Err(err) => {
            return render_error(DpcError::Config(err.to_string()), format, output.clone())
        }
    };
    if verbose {
        eprintln!(
            "Normalized input ({:?}); generate-code is currently stubbed",
            input_res.kind
        );
    }
    let body = DpcOutput::GenerateCode(GenerateCodeOutput {
        version: DPC_OUTPUT_VERSION.to_string(),
        input: ResourceDescriptor {
            kind: input_res.kind,
            value: input_res.value,
        },
        viewport,
        stack: Some(stack),
        output_path: output.clone(),
        code: None,
        summary: Some(Summary {
            top_issues: vec![
                String::from(
                    "Not implemented: generate-code will return code later; for now, use an external screenshot-to-code service and run `dpc compare` for parity checks.",
                ),
                String::from(
                    "Next steps: keep artifacts with --keep-artifacts/--artifacts-dir for handoff to codegen tools.",
                ),
            ],
        }),
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
