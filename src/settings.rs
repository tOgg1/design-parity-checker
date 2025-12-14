use std::path::Path;

use dpc_lib::types::Viewport;
use dpc_lib::{Config, DpcError, ScoreWeights};

/// Tracks which CLI flags were explicitly provided vs. defaulted.
#[derive(Debug, Default)]
pub struct CompareFlagSources {
    pub viewport: bool,
    pub threshold: bool,
    pub nav_timeout: bool,
    pub network_idle_timeout: bool,
    pub process_timeout: bool,
}

impl CompareFlagSources {
    pub fn from_args(args: &[String]) -> Self {
        Self {
            viewport: flag_present(args, "--viewport"),
            threshold: flag_present(args, "--threshold"),
            nav_timeout: flag_present(args, "--nav-timeout"),
            network_idle_timeout: flag_present(args, "--network-idle-timeout"),
            process_timeout: flag_present(args, "--process-timeout"),
        }
    }
}

/// Checks if a flag was present in the command-line arguments.
pub fn flag_present(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|arg| arg == flag || arg.starts_with(&format!("{flag}=")))
}

/// Resolved settings after merging CLI args and config file.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedCompareSettings {
    pub viewport: Viewport,
    pub threshold: f64,
    pub nav_timeout: u64,
    pub network_idle_timeout: u64,
    pub process_timeout: u64,
    pub weights: ScoreWeights,
}

/// Merge CLI arguments with config file, preferring CLI when flags are present.
pub fn resolve_compare_settings(
    cli_viewport: Viewport,
    cli_threshold: f64,
    cli_nav_timeout: u64,
    cli_network_idle_timeout: u64,
    cli_process_timeout: u64,
    config: &Config,
    flags: &CompareFlagSources,
) -> ResolvedCompareSettings {
    let weights = ScoreWeights {
        pixel: config.metric_weights.pixel,
        layout: config.metric_weights.layout,
        typography: config.metric_weights.typography,
        color: config.metric_weights.color,
        content: config.metric_weights.content,
    };

    ResolvedCompareSettings {
        viewport: if flags.viewport {
            cli_viewport
        } else {
            config.viewport
        },
        threshold: if flags.threshold {
            cli_threshold
        } else {
            config.threshold
        },
        nav_timeout: if flags.nav_timeout {
            cli_nav_timeout
        } else {
            config.timeouts.navigation.as_secs()
        },
        network_idle_timeout: if flags.network_idle_timeout {
            cli_network_idle_timeout
        } else {
            config.timeouts.network_idle.as_secs()
        },
        process_timeout: if flags.process_timeout {
            cli_process_timeout
        } else {
            config.timeouts.process.as_secs()
        },
        weights,
    }
}

/// Load config from a TOML file, central config, or return defaults.
/// Priority: explicit path > ~/.config/dpc/config.toml > defaults
pub fn load_config(path: Option<&Path>) -> Result<Config, DpcError> {
    let cfg = Config::load(path).map_err(|e| {
        let loc = path
            .map(|p| p.display().to_string())
            .or_else(|| Config::central_config_path().map(|p| p.display().to_string()))
            .unwrap_or_else(|| "defaults".to_string());
        DpcError::Config(format!("Failed to read config {}: {}", loc, e))
    })?;

    cfg.validate().map_err(|e| {
        let prefix = path
            .map(|p| format!("Invalid config ({}): {}", p.display(), e))
            .unwrap_or_else(|| format!("Invalid config: {}", e));
        DpcError::Config(prefix)
    })?;
    Ok(cfg)
}

/// Log effective config to stderr (verbose mode).
pub fn log_effective_config(
    config_path: Option<&Path>,
    viewport: &Viewport,
    threshold: f64,
    weights: &ScoreWeights,
    nav_timeout: u64,
    network_idle_timeout: u64,
    process_timeout: u64,
) {
    let config_source = config_path
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "defaults/built-in".to_string());
    eprintln!(
        "Effective config (source: {}): viewport {}x{}, threshold {:.2}, timeouts nav {}s / idle {}s / process {}s, weights pixel {:.2}, layout {:.2}, typography {:.2}, color {:.2}, content {:.2}",
        config_source,
        viewport.width,
        viewport.height,
        threshold,
        nav_timeout,
        network_idle_timeout,
        process_timeout,
        weights.pixel,
        weights.layout,
        weights.typography,
        weights.color,
        weights.content
    );
}

/// Format effective config as a single-line string.
pub fn format_effective_config(
    viewport: &Viewport,
    threshold: f64,
    nav_timeout: u64,
    network_idle_timeout: u64,
    process_timeout: u64,
    weights: &ScoreWeights,
    config_source: Option<&Path>,
) -> String {
    let source = config_source
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "defaults".to_string());
    format!(
        "Effective config [{source}]: viewport={}x{}, threshold={:.2}, timeouts: nav={}s, network-idle={}s, process={}s, weights: pixel={:.2}, layout={:.2}, typography={:.2}, color={:.2}, content={:.2}",
        viewport.width,
        viewport.height,
        threshold,
        nav_timeout,
        network_idle_timeout,
        process_timeout,
        weights.pixel,
        weights.layout,
        weights.typography,
        weights.color,
        weights.content
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpc_lib::config::{MetricWeights, SemanticConfig, Timeouts};
    use std::time::Duration;

    #[test]
    fn resolve_compare_settings_prefers_config_when_flags_absent() {
        let cfg = Config {
            viewport: Viewport {
                width: 111,
                height: 222,
            },
            threshold: 0.5,
            metric_weights: MetricWeights {
                pixel: 1.0,
                layout: 2.0,
                typography: 3.0,
                color: 4.0,
                content: 5.0,
            },
            timeouts: Timeouts {
                navigation: Duration::from_secs(5),
                network_idle: Duration::from_secs(6),
                process: Duration::from_secs(7),
            },
            semantic: SemanticConfig::default(),
        };
        let flags = CompareFlagSources::default();
        let resolved = resolve_compare_settings(
            Viewport {
                width: 999,
                height: 999,
            },
            0.9,
            30,
            10,
            45,
            &cfg,
            &flags,
        );

        assert_eq!(resolved.viewport.width, 111);
        assert_eq!(resolved.viewport.height, 222);
        assert_eq!(resolved.threshold, 0.5);
        assert_eq!(resolved.nav_timeout, 5);
        assert_eq!(resolved.network_idle_timeout, 6);
        assert_eq!(resolved.process_timeout, 7);
        assert!((resolved.weights.pixel - 1.0).abs() < f32::EPSILON);
        assert!((resolved.weights.content - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_compare_settings_prefers_cli_when_flags_present() {
        let cfg = Config::default();
        let flags = CompareFlagSources {
            viewport: true,
            threshold: true,
            nav_timeout: true,
            network_idle_timeout: true,
            process_timeout: true,
        };
        let resolved = resolve_compare_settings(
            Viewport {
                width: 10,
                height: 20,
            },
            0.9,
            50,
            60,
            70,
            &cfg,
            &flags,
        );

        assert_eq!(resolved.viewport.width, 10);
        assert_eq!(resolved.viewport.height, 20);
        assert_eq!(resolved.threshold, 0.9);
        assert_eq!(resolved.nav_timeout, 50);
        assert_eq!(resolved.network_idle_timeout, 60);
        assert_eq!(resolved.process_timeout, 70);
    }

    #[test]
    fn format_effective_config_includes_all_fields() {
        let summary = format_effective_config(
            &Viewport {
                width: 1280,
                height: 720,
            },
            0.9,
            12,
            8,
            45,
            &ScoreWeights {
                pixel: 0.3,
                layout: 0.25,
                typography: 0.2,
                color: 0.15,
                content: 0.1,
            },
            Some(Path::new("dpc.toml")),
        );
        assert!(summary.contains("1280x720"));
        assert!(summary.contains("threshold=0.90"));
        assert!(summary.contains("nav=12s"));
        assert!(summary.contains("network-idle=8s"));
        assert!(summary.contains("process=45s"));
        assert!(summary.contains("pixel=0.30"));
        assert!(summary.contains("layout=0.25"));
        assert!(summary.contains("typography=0.20"));
        assert!(summary.contains("color=0.15"));
        assert!(summary.contains("content=0.10"));
        assert!(summary.contains("dpc.toml"));
    }
}
