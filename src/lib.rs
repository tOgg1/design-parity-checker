//! Design Parity Checker (DPC) Library
//!
//! A library for comparing design references against implementations to measure
//! visual parity. Supports multiple input types: URLs (via Playwright), Figma designs,
//! and local images.
//!
//! # Module Overview
//!
//! - [`browser`] - Headless browser automation for URL capture
//! - [`figma`] - Figma API integration and design extraction
//! - [`image_loader`] - Local image loading and processing
//! - [`metrics`] - Parity metrics (pixel, layout, typography, color, content)
//! - [`config`] - Configuration file support
//! - [`types`] - Core data types and structures
//! - [`output`] - JSON output schemas
//!
//! # Example
//!
//! ```no_run
//! use dpc_lib::{BrowserManager, BrowserOptions, image_to_normalized_view};
//! use dpc_lib::{default_metrics, run_metrics, MetricKind, calculate_combined_score, ScoreWeights};
//!
//! # async fn example() -> dpc_lib::Result<()> {
//! // Capture a URL
//! let manager = BrowserManager::new(BrowserOptions::default());
//! let result = manager.render_url("https://example.com", None).await?;
//!
//! // Run metrics comparison
//! let metrics = default_metrics();
//! let selected = vec![MetricKind::Pixel, MetricKind::Color];
//! // ... compare views and compute scores
//! # Ok(())
//! # }
//! ```

pub mod browser;
pub mod config;
pub mod error;
#[path = "figma/mod.rs"]
pub mod figma;
pub mod figma_client;
pub mod image_loader;
pub mod metrics;
pub mod output;
pub mod resource;
pub mod types;
pub mod viewport;

// Browser module re-exports
pub use browser::{
    url_to_normalized_view, BrowserManager, BrowserOptions, PageRenderResult, UrlToViewOptions,
    DEFAULT_NAVIGATION_TIMEOUT, DEFAULT_NETWORK_IDLE_TIMEOUT, DEFAULT_PROCESS_TIMEOUT,
};
pub use config::Config;
pub use error::{DpcError, Result};
pub use figma::{figma_to_normalized_view, FigmaClient, FigmaError, FigmaRenderOptions};
pub use figma_client::{
    FigmaApiClient, FigmaAuth, FigmaFileResponse, FigmaImageFormat, FigmaImageResponse,
    FigmaNodesResponse, ImageExportOptions,
};
pub use image_loader::{image_to_normalized_view, load_image, ImageLoadOptions};
// Metrics module re-exports
pub use metrics::{
    // Core traits and types
    calculate_combined_score,
    // Concrete metric implementations (for custom configuration)
    cluster_diff_regions,
    default_metrics,
    generate_top_issues,
    run_metrics,
    ColorPaletteMetric,
    ContentSimilarity,
    LayoutSimilarity,
    Metric,
    MetricKind,
    MetricResult,
    PixelDiffThresholds,
    PixelSimilarity,
    ScoreWeights,
    TypographySimilarity,
};
pub use output::{
    CompareArtifacts, CompareOutput, DpcOutput, ErrorOutput, FindingSeverity, GenerateCodeOutput,
    QualityFinding, QualityFindingType, QualityOutput, ResourceDescriptor, Summary,
    DPC_OUTPUT_VERSION,
};
pub use resource::{parse_resource, FigmaInfo, ParsedResource};
pub use types::{
    ColorMetric, ContentMetric, LayoutMetric, MetricScores, NormalizedView, PixelMetric,
    ResourceKind, TypographyMetric,
};
pub use viewport::Viewport;
