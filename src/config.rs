use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::Viewport;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(
        default = "Viewport::default",
        deserialize_with = "deserialize_viewport"
    )]
    pub viewport: Viewport,
    pub threshold: f64,
    pub metric_weights: MetricWeights,
    pub timeouts: Timeouts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricWeights {
    pub pixel: f32,
    pub layout: f32,
    pub typography: f32,
    pub color: f32,
    pub content: f32,
}

impl Default for MetricWeights {
    fn default() -> Self {
        Self {
            // Match ScoreWeights::default() from metrics to keep existing weighting behavior.
            pixel: 0.35,
            layout: 0.25,
            typography: 0.15,
            color: 0.15,
            content: 0.10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeouts {
    #[serde(with = "humantime_serde")]
    pub navigation: Duration,
    #[serde(with = "humantime_serde")]
    pub network_idle: Duration,
    #[serde(with = "humantime_serde")]
    pub process: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            navigation: Duration::from_secs(30),
            network_idle: Duration::from_secs(10),
            process: Duration::from_secs(45),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            viewport: Viewport::default(),
            threshold: 0.95,
            metric_weights: MetricWeights::default(),
            timeouts: Timeouts::default(),
        }
    }
}

fn deserialize_viewport<'de, D>(deserializer: D) -> Result<Viewport, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ViewportToml {
        String(String),
        Table { width: u32, height: u32 },
    }

    match ViewportToml::deserialize(deserializer)? {
        ViewportToml::String(s) => Viewport::from_str(&s).map_err(de::Error::custom),
        ViewportToml::Table { width, height } => {
            if width == 0 || height == 0 {
                return Err(de::Error::custom(
                    "viewport width and height must be greater than zero",
                ));
            }
            Ok(Viewport { width, height })
        }
    }
}

impl Config {
    /// Load configuration from a TOML file. Missing fields fall back to defaults.
    pub fn from_toml_file(path: &Path) -> Result<Self, std::io::Error> {
        let contents = fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&contents)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        cfg.apply_defaults();
        Ok(cfg)
    }

    /// Ensure defaults are applied when deserializing partial configs.
    fn apply_defaults(&mut self) {
        let defaults = Config::default();
        if self.threshold < 0.0 {
            self.threshold = defaults.threshold;
        }
        self.metric_weights = MetricWeights {
            pixel: if self.metric_weights.pixel <= 0.0 {
                defaults.metric_weights.pixel
            } else {
                self.metric_weights.pixel
            },
            layout: if self.metric_weights.layout <= 0.0 {
                defaults.metric_weights.layout
            } else {
                self.metric_weights.layout
            },
            typography: if self.metric_weights.typography <= 0.0 {
                defaults.metric_weights.typography
            } else {
                self.metric_weights.typography
            },
            color: if self.metric_weights.color <= 0.0 {
                defaults.metric_weights.color
            } else {
                self.metric_weights.color
            },
            content: if self.metric_weights.content <= 0.0 {
                defaults.metric_weights.content
            } else {
                self.metric_weights.content
            },
        };
        self.timeouts = Timeouts {
            navigation: if self.timeouts.navigation == Duration::from_secs(0) {
                defaults.timeouts.navigation
            } else {
                self.timeouts.navigation
            },
            network_idle: if self.timeouts.network_idle == Duration::from_secs(0) {
                defaults.timeouts.network_idle
            } else {
                self.timeouts.network_idle
            },
            process: if self.timeouts.process == Duration::from_secs(0) {
                defaults.timeouts.process
            } else {
                self.timeouts.process
            },
        };
    }

    /// Validate thresholds and weights to ensure reasonable ranges.
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.threshold) {
            return Err("threshold must be between 0.0 and 1.0".to_string());
        }
        let weights = [
            self.metric_weights.pixel,
            self.metric_weights.layout,
            self.metric_weights.typography,
            self.metric_weights.color,
            self.metric_weights.content,
        ];
        if weights.iter().any(|w| *w <= 0.0) {
            return Err("all metric weights must be positive".to_string());
        }
        if self.timeouts.navigation.is_zero()
            || self.timeouts.network_idle.is_zero()
            || self.timeouts.process.is_zero()
        {
            return Err("timeouts must be greater than zero seconds".to_string());
        }
        if self.viewport.width == 0 || self.viewport.height == 0 {
            return Err("viewport width and height must be greater than zero".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, MetricWeights, Timeouts};
    use crate::Viewport;
    use std::time::Duration;

    #[test]
    fn default_values_match_expected() {
        let cfg = Config::default();

        assert_eq!(cfg.viewport.width, 1440);
        assert_eq!(cfg.viewport.height, 900);
        assert!((cfg.threshold - 0.95).abs() < f64::EPSILON);
        assert!((cfg.metric_weights.pixel - 0.35).abs() < f32::EPSILON);
        assert!((cfg.metric_weights.layout - 0.25).abs() < f32::EPSILON);
        assert_eq!(cfg.timeouts.navigation, Duration::from_secs(30));
        assert_eq!(cfg.timeouts.network_idle, Duration::from_secs(10));
        assert_eq!(cfg.timeouts.process, Duration::from_secs(45));
    }

    #[test]
    fn can_override_weights_and_timeouts() {
        let cfg = Config {
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            threshold: 0.9,
            metric_weights: MetricWeights {
                pixel: 0.5,
                layout: 1.2,
                typography: 1.0,
                color: 0.8,
                content: 0.7,
            },
            timeouts: Timeouts {
                navigation: Duration::from_secs(20),
                network_idle: Duration::from_secs(5),
                process: Duration::from_secs(60),
            },
        };

        assert_eq!(cfg.viewport.width, 1280);
        assert_eq!(cfg.viewport.height, 720);
        assert!((cfg.threshold - 0.9).abs() < f64::EPSILON);
        assert!((cfg.metric_weights.pixel - 0.5).abs() < f32::EPSILON);
        assert!((cfg.metric_weights.layout - 1.2).abs() < f32::EPSILON);
        assert_eq!(cfg.timeouts.navigation, Duration::from_secs(20));
        assert_eq!(cfg.timeouts.network_idle, Duration::from_secs(5));
        assert_eq!(cfg.timeouts.process, Duration::from_secs(60));
    }

    #[test]
    fn validate_rejects_bad_threshold_and_weights() {
        let mut cfg = Config::default();
        cfg.threshold = -0.1;
        assert!(cfg.validate().is_err());

        cfg.threshold = 0.9;
        cfg.metric_weights.pixel = 0.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn load_from_toml_applies_defaults() {
        let tmp = tempfile::Builder::new()
            .suffix(".toml")
            .tempfile()
            .expect("temp file");
        std::fs::write(
            tmp.path(),
            r#"
threshold = 0.9
[metric_weights]
pixel = 0.8
layout = 0.0 # should default
# durations in human form
[timeouts]
navigation = "20s"
network_idle = "5s"
process = "55s"
"#,
        )
        .unwrap();

        let cfg = Config::from_toml_file(tmp.path()).expect("load config");
        assert!((cfg.threshold - 0.9).abs() < f64::EPSILON);
        assert!((cfg.metric_weights.pixel - 0.8).abs() < f32::EPSILON);
        assert!((cfg.metric_weights.layout - MetricWeights::default().layout).abs() < f32::EPSILON);
        assert_eq!(cfg.viewport.width, 1440);
        assert_eq!(cfg.viewport.height, 900);
        assert_eq!(cfg.timeouts.navigation, Duration::from_secs(20));
        assert_eq!(cfg.timeouts.network_idle, Duration::from_secs(5));
        assert_eq!(cfg.timeouts.process, Duration::from_secs(55));
    }

    #[test]
    fn load_from_toml_accepts_viewport_string() {
        let tmp = tempfile::Builder::new()
            .suffix(".toml")
            .tempfile()
            .expect("temp file");
        std::fs::write(
            tmp.path(),
            r#"
viewport = "1024x768"
threshold = 0.8
[timeouts]
navigation = "10s"
network_idle = "5s"
process = "15s"
"#,
        )
        .unwrap();

        let cfg = Config::from_toml_file(tmp.path()).expect("load config");
        assert_eq!(cfg.viewport.width, 1024);
        assert_eq!(cfg.viewport.height, 768);
        assert!((cfg.threshold - 0.8).abs() < f64::EPSILON);
        assert_eq!(cfg.timeouts.navigation, Duration::from_secs(10));
    }

    #[test]
    fn validate_rejects_zero_viewport_dimensions() {
        let cfg = Config {
            viewport: Viewport {
                width: 0,
                height: 0,
            },
            ..Config::default()
        };

        assert!(cfg.validate().is_err());
    }
}
