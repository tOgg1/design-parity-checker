pub mod browser;
pub mod config;
pub mod error;
pub mod figma;
pub mod figma_client;
pub mod image_loader;
pub mod metrics;
pub mod output;
pub mod resource;
pub mod types;
pub mod viewport;

pub use browser::{
    url_to_normalized_view, BrowserManager, BrowserOptions, PageRenderResult, UrlToViewOptions,
};
pub use config::Config;
pub use error::{DpcError, Result};
pub use figma::{figma_to_normalized_view, FigmaClient, FigmaError, FigmaRenderOptions};
pub use figma_client::{
    FigmaApiClient, FigmaAuth, FigmaFileResponse, FigmaImageFormat, FigmaImageResponse,
    FigmaNodesResponse, ImageExportOptions,
};
pub use image_loader::{image_to_normalized_view, load_image, ImageLoadOptions};
pub use metrics::{
    calculate_combined_score, default_metrics, generate_top_issues, run_metrics, Metric,
    MetricKind, MetricResult, ScoreWeights,
};
pub use output::{
    CompareOutput, DpcOutput, FindingSeverity, GenerateCodeOutput, QualityFinding, QualityOutput,
    ResourceDescriptor, Summary,
};
pub use resource::{parse_resource, FigmaInfo, ParsedResource};
pub use types::{
    ColorMetric, ContentMetric, LayoutMetric, MetricScores, NormalizedView, PixelMetric,
    ResourceKind, TypographyMetric,
};
pub use viewport::Viewport;
