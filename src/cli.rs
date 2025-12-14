use clap::{Parser, Subcommand, ValueEnum};
use dpc_lib::Viewport;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dpc")]
#[command(
    version,
    about = "Design Parity Checker - Compare implementations against design references",
    long_about = "Design Parity Checker (DPC)\n\nModes:\n- compare: measure similarity between a reference (Figma/URL/image) and an implementation (Figma/URL/image).\n- generate-code: stub for future code generation.\n- quality: experimental reference-free scoring.\n\nUse --help on any subcommand for details."
)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true, help = "Enable verbose output")]
    pub verbose: bool,

    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Optional config file (TOML) to set defaults for viewport/threshold/weights/timeouts"
    )]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compare a reference design against an implementation
    Compare {
        #[arg(long, help = "Reference resource (Figma URL, web URL, or local image)")]
        r#ref: String,

        #[arg(
            long,
            help = "Implementation resource (Figma URL, web URL, or local image)"
        )]
        r#impl: String,

        #[arg(long, value_enum, help = "Override type detection for reference")]
        ref_type: Option<ResourceType>,

        #[arg(long, value_enum, help = "Override type detection for implementation")]
        impl_type: Option<ResourceType>,

        #[arg(
            long,
            default_value = "1440x900",
            help = "Viewport dimensions (WIDTHxHEIGHT)"
        )]
        viewport: Viewport,

        #[arg(
            long,
            default_value = "0.95",
            help = "Similarity threshold for pass/fail (similarity >= threshold passes)"
        )]
        threshold: f64,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Metrics to compute (pixel,layout,typography,color,content)"
        )]
        metrics: Option<Vec<String>>,

        #[arg(
            long,
            help = "CSS selectors to ignore in DOM comparisons (comma-separated; supports #id, .class, tag)"
        )]
        ignore_selectors: Option<String>,

        #[arg(
            long,
            help = "Path to JSON array of {x,y,width,height} regions to mask before metrics (values can be px or 0-1 normalized)"
        )]
        ignore_regions: Option<PathBuf>,

        #[arg(long, value_enum, default_value = "json", help = "Output format")]
        format: OutputFormat,

        #[arg(long, short, help = "Output file path (stdout if omitted)")]
        output: Option<PathBuf>,

        #[arg(
            long,
            help = "Keep intermediate artifacts (screenshots, DOM snapshots); otherwise cleaned up"
        )]
        keep_artifacts: bool,

        #[arg(
            long,
            help = "Directory to store artifacts (implies --keep-artifacts); created if missing",
            value_name = "PATH"
        )]
        artifacts_dir: Option<PathBuf>,

        #[arg(
            long,
            default_value = "30",
            help = "Navigation timeout (seconds) for URL rendering"
        )]
        nav_timeout: u64,

        #[arg(
            long,
            default_value = "10",
            help = "Network idle timeout (seconds) for URL rendering"
        )]
        network_idle_timeout: u64,

        #[arg(
            long,
            default_value = "45",
            help = "Process timeout (seconds) for Playwright invocation"
        )]
        process_timeout: u64,
    },

    /// Generate HTML/Tailwind code from a design input
    GenerateCode {
        #[arg(long, help = "Input resource (Figma URL, web URL, or local image)")]
        input: String,

        #[arg(long, value_enum, help = "Override type detection for input")]
        input_type: Option<ResourceType>,

        #[arg(
            long,
            default_value = "html+tailwind",
            help = "Output stack (e.g., html+tailwind)"
        )]
        stack: String,

        #[arg(
            long,
            default_value = "1440x900",
            help = "Viewport dimensions (WIDTHxHEIGHT)"
        )]
        viewport: Viewport,

        #[arg(long, short, help = "Output file path")]
        output: Option<PathBuf>,

        #[arg(long, value_enum, default_value = "json", help = "Output format")]
        format: OutputFormat,
    },

    /// Compute reference-free design quality score (experimental)
    Quality {
        #[arg(long, help = "Input resource (Figma URL, web URL, or local image)")]
        input: String,

        #[arg(long, value_enum, help = "Override type detection for input")]
        input_type: Option<ResourceType>,

        #[arg(
            long,
            default_value = "1440x900",
            help = "Viewport dimensions (WIDTHxHEIGHT)"
        )]
        viewport: Viewport,

        #[arg(long, short, help = "Output file path")]
        output: Option<PathBuf>,

        #[arg(long, value_enum, default_value = "json", help = "Output format")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ResourceType {
    Url,
    Image,
    Figma,
}

#[derive(Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Json,
    Pretty,
}

pub fn parse() -> Cli {
    Cli::parse()
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, OutputFormat, ResourceType};
    use clap::Parser;

    #[test]
    fn compare_command_uses_defaults() {
        let cli = Cli::parse_from([
            "dpc",
            "compare",
            "--ref",
            "https://example.com/design",
            "--impl",
            "https://example.com/build",
        ]);

        assert!(!cli.verbose);
        assert!(cli.config.is_none());

        match cli.command {
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
                ignore_selectors,
                ignore_regions,
                keep_artifacts,
                artifacts_dir,
                nav_timeout,
                network_idle_timeout,
                process_timeout,
            } => {
                assert_eq!(r#ref, "https://example.com/design");
                assert_eq!(r#impl, "https://example.com/build");
                assert!(ref_type.is_none());
                assert!(impl_type.is_none());
                assert_eq!(viewport.width, 1440);
                assert_eq!(viewport.height, 900);
                assert!((threshold - 0.95).abs() < f64::EPSILON);
                assert!(metrics.is_none());
                assert!(matches!(format, OutputFormat::Json));
                assert!(output.is_none());
                assert!(ignore_selectors.is_none());
                assert!(ignore_regions.is_none());
                assert!(!keep_artifacts);
                assert!(artifacts_dir.is_none());
                assert_eq!(nav_timeout, 30);
                assert_eq!(network_idle_timeout, 10);
                assert_eq!(process_timeout, 45);
            }
            _ => panic!("expected compare command"),
        }
    }

    #[test]
    fn compare_command_respects_overrides() {
        let cli = Cli::parse_from([
            "dpc",
            "compare",
            "--ref",
            "ref.png",
            "--impl",
            "impl.png",
            "--ref-type",
            "image",
            "--impl-type",
            "figma",
            "--viewport",
            "1920x1080",
            "--threshold",
            "0.9",
            "--metrics",
            "pixel,layout",
            "--format",
            "pretty",
            "--output",
            "report.json",
            "--ignore-selectors",
            ".ads,.tracking",
            "--ignore-regions",
            "regions.json",
            "--keep-artifacts",
            "--artifacts-dir",
            "artifacts",
            "--nav-timeout",
            "20",
            "--network-idle-timeout",
            "6",
            "--process-timeout",
            "50",
            "--config",
            "dpc.toml",
        ]);

        match cli.command {
            Commands::Compare {
                ref_type,
                impl_type,
                viewport,
                threshold,
                metrics,
                format,
                output,
                ignore_selectors,
                ignore_regions,
                keep_artifacts,
                artifacts_dir,
                nav_timeout,
                network_idle_timeout,
                process_timeout,
                ..
            } => {
                assert!(matches!(ref_type, Some(ResourceType::Image)));
                assert!(matches!(impl_type, Some(ResourceType::Figma)));
                assert_eq!(viewport.width, 1920);
                assert_eq!(viewport.height, 1080);
                assert!((threshold - 0.9).abs() < f64::EPSILON);
                assert_eq!(
                    metrics,
                    Some(vec![String::from("pixel"), String::from("layout"),])
                );
                assert!(matches!(format, OutputFormat::Pretty));
                assert_eq!(output.as_deref(), Some(std::path::Path::new("report.json")));
                assert_eq!(ignore_selectors.as_deref(), Some(".ads,.tracking"));
                assert_eq!(
                    ignore_regions.as_deref(),
                    Some(std::path::Path::new("regions.json"))
                );
                assert!(keep_artifacts);
                assert_eq!(
                    artifacts_dir.as_deref(),
                    Some(std::path::Path::new("artifacts"))
                );
                assert_eq!(nav_timeout, 20);
                assert_eq!(network_idle_timeout, 6);
                assert_eq!(process_timeout, 50);
            }
            _ => panic!("expected compare command with overrides"),
        }
    }

    #[test]
    fn quality_command_sets_verbose() {
        let cli = Cli::parse_from([
            "dpc",
            "--verbose",
            "quality",
            "--input",
            "https://example.com/page",
        ]);

        assert!(cli.verbose);

        match cli.command {
            Commands::Quality {
                input,
                format,
                output,
                input_type,
                viewport,
            } => {
                assert_eq!(input, "https://example.com/page");
                assert!(matches!(format, OutputFormat::Json));
                assert!(output.is_none());
                assert!(input_type.is_none());
                assert_eq!(viewport.width, 1440);
                assert_eq!(viewport.height, 900);
            }
            _ => panic!("expected quality command"),
        }
    }
}
