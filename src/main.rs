mod cli;
mod commands;
mod formatting;
mod pipeline;
mod settings;

use std::process::ExitCode;

use cli::Commands;
use commands::{run_compare, run_generate_code, run_quality};

#[tokio::main]
async fn main() -> ExitCode {
    run().await
}

async fn run() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().collect();
    let args = cli::parse();

    match args.command {
        Commands::Compare {
            r#ref,
            r#impl,
            ref_type,
            impl_type,
            viewport,
            threshold,
            metrics,
            format,
            output,
            keep_artifacts,
            ignore_selectors,
            ignore_regions,
            artifacts_dir,
            nav_timeout,
            network_idle_timeout,
            process_timeout,
            semantic_analysis,
            context,
        } => {
            run_compare(
                &raw_args,
                args.config,
                args.verbose,
                r#ref,
                r#impl,
                ref_type,
                impl_type,
                viewport,
                threshold,
                metrics,
                format,
                output,
                keep_artifacts,
                ignore_selectors,
                ignore_regions,
                artifacts_dir,
                nav_timeout,
                network_idle_timeout,
                process_timeout,
                semantic_analysis,
                context,
            )
            .await
        }
        Commands::GenerateCode {
            input,
            input_type,
            viewport,
            stack,
            output,
            format,
        } => {
            run_generate_code(
                &raw_args,
                args.config,
                args.verbose,
                input,
                input_type,
                viewport,
                stack,
                output,
                format,
            )
            .await
        }
        Commands::Quality {
            input,
            input_type,
            viewport,
            format,
            output,
        } => {
            run_quality(
                &raw_args,
                args.config,
                args.verbose,
                input,
                input_type,
                viewport,
                format,
                output,
            )
            .await
        }
    }
}
