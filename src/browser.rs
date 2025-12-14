use crate::types::{
    BoundingBox, ComputedStyle, DomNode, DomSnapshot, NormalizedView, ResourceKind,
};
use crate::{DpcError, Result, Viewport};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::timeout;

const PLAYWRIGHT_SCRIPT: &str = r#"
const [, , url, width, height, navTimeout, idleTimeout, screenshotPath, headlessFlag] = process.argv;

async function run() {
  let browser;
  try {
    const { chromium } = require('playwright');
    browser = await chromium.launch({ headless: headlessFlag !== '0' });
    const context = await browser.newContext({
      viewport: {
        width: parseInt(width, 10),
        height: parseInt(height, 10)
      }
    });
    const page = await context.newPage();
    const navMs = parseInt(navTimeout, 10);
    const idleMs = parseInt(idleTimeout, 10);

    await page.goto(url, { waitUntil: 'networkidle', timeout: navMs });
    await page.waitForLoadState('networkidle', { timeout: idleMs });

    if (screenshotPath) {
      await page.screenshot({ path: screenshotPath, fullPage: true });
    }

    console.log(JSON.stringify({ status: 'ok' }));
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    console.error(JSON.stringify({ status: 'error', message }));
    process.exitCode = 1;
  } finally {
    if (browser) {
      await browser.close();
    }
  }
}

run();
"#;

const PLAYWRIGHT_SCRIPT_WITH_DOM: &str = r#"
const [, , url, width, height, navTimeout, idleTimeout, screenshotPath, headlessFlag] = process.argv;

async function run() {
  let browser;
  try {
    const { chromium } = require('playwright');
    browser = await chromium.launch({ headless: headlessFlag !== '0' });
    const context = await browser.newContext({
      viewport: {
        width: parseInt(width, 10),
        height: parseInt(height, 10)
      }
    });
    const page = await context.newPage();
    const navMs = parseInt(navTimeout, 10);
    const idleMs = parseInt(idleTimeout, 10);

    await page.goto(url, { waitUntil: 'networkidle', timeout: navMs });
    await page.waitForLoadState('networkidle', { timeout: idleMs });

    if (screenshotPath) {
      await page.screenshot({ path: screenshotPath, fullPage: false });
    }

    // Extract DOM snapshot
    const domSnapshot = await page.evaluate(() => {
      const nodes = [];
      let nodeId = 0;
      const nodeMap = new Map();

      function getComputedStyleInfo(el) {
        const style = window.getComputedStyle(el);
        return {
          fontFamily: style.fontFamily || null,
          fontSize: parseFloat(style.fontSize) || null,
          fontWeight: style.fontWeight || null,
          lineHeight: parseFloat(style.lineHeight) || null,
          color: style.color || null,
          backgroundColor: style.backgroundColor || null,
          display: style.display || null,
          visibility: style.visibility || null,
          opacity: style.opacity !== '' ? parseFloat(style.opacity) : null
        };
      }

      function traverse(node, parentId) {
        if (node.nodeType !== Node.ELEMENT_NODE) return null;

        const el = node;
        const id = `node-${nodeId++}`;
        nodeMap.set(el, id);

        const rect = el.getBoundingClientRect();
        const tag = el.tagName.toLowerCase();

        // Skip invisible elements
        if (rect.width === 0 && rect.height === 0) return null;

        const attributes = {};
        for (const attr of el.attributes) {
          attributes[attr.name] = attr.value;
        }

        // Get direct text content (not from children)
        let text = null;
        for (const child of el.childNodes) {
          if (child.nodeType === Node.TEXT_NODE) {
            const trimmed = child.textContent.trim();
            if (trimmed) {
              text = text ? text + ' ' + trimmed : trimmed;
            }
          }
        }

        const childIds = [];
        for (const child of el.children) {
          const childId = traverse(child, id);
          if (childId) childIds.push(childId);
        }

        nodes.push({
          id,
          tag,
          children: childIds,
          parent: parentId,
          attributes,
          text,
          boundingBox: {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height
          },
          computedStyle: getComputedStyleInfo(el)
        });

        return id;
      }

      traverse(document.body, null);

      return {
        url: window.location.href,
        title: document.title,
        nodes
      };
    });

    console.log(JSON.stringify({ status: 'ok', dom: domSnapshot }));
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    console.error(JSON.stringify({ status: 'error', message }));
    process.exitCode = 1;
  } finally {
    if (browser) {
      await browser.close();
    }
  }
}

run();
"#;

const DEFAULT_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PROCESS_TIMEOUT: Duration = Duration::from_secs(45);
const NODE_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct BrowserOptions {
    pub node_command: String,
    pub viewport: Viewport,
    pub headless: bool,
    pub navigation_timeout: Duration,
    pub network_idle_timeout: Duration,
    pub process_timeout: Duration,
    pub max_concurrent_sessions: usize,
}

impl Default for BrowserOptions {
    fn default() -> Self {
        Self {
            node_command: "node".to_string(),
            viewport: Viewport::default(),
            headless: true,
            navigation_timeout: DEFAULT_NAVIGATION_TIMEOUT,
            network_idle_timeout: DEFAULT_NETWORK_IDLE_TIMEOUT,
            process_timeout: DEFAULT_PROCESS_TIMEOUT,
            max_concurrent_sessions: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserManager {
    options: BrowserOptions,
    semaphore: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
pub struct PageRenderResult {
    pub screenshot_path: Option<PathBuf>,
    pub viewport: Viewport,
    pub elapsed: Duration,
}

impl BrowserManager {
    pub fn new(options: BrowserOptions) -> Self {
        let permits = options.max_concurrent_sessions.max(1);
        Self {
            options,
            semaphore: Arc::new(Semaphore::new(permits)),
        }
    }

    pub async fn render_url(
        &self,
        url: &str,
        screenshot_path: Option<&Path>,
    ) -> Result<PageRenderResult> {
        self.ensure_node_available().await?;
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| DpcError::Config("Browser manager unavailable".to_string()))?;

        self.run_playwright(url, screenshot_path).await
    }

    /// Render a URL to a full NormalizedView (screenshot + DOM snapshot) using the manager's settings.
    pub async fn render_url_to_normalized_view(
        &self,
        url: &str,
        screenshot_path: &Path,
    ) -> Result<NormalizedView> {
        self.ensure_node_available().await?;
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| DpcError::Config("Browser manager unavailable".to_string()))?;

        let options: UrlToViewOptions = self.options.clone().into();
        url_to_normalized_view(url, screenshot_path, options).await
    }

    async fn run_playwright(
        &self,
        url: &str,
        screenshot_path: Option<&Path>,
    ) -> Result<PageRenderResult> {
        let mut cmd = Command::new(&self.options.node_command);
        cmd.arg("-e")
            .arg(PLAYWRIGHT_SCRIPT)
            .arg(url)
            .arg(self.options.viewport.width.to_string())
            .arg(self.options.viewport.height.to_string())
            .arg(self.options.navigation_timeout.as_millis().to_string())
            .arg(self.options.network_idle_timeout.as_millis().to_string())
            .arg(
                screenshot_path
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_default(),
            )
            .arg(if self.options.headless { "1" } else { "0" })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let start = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|err| map_spawn_error(err, &self.options.node_command))?;

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut out) = stdout_pipe {
                let _ = out.read_to_end(&mut buf).await;
            }
            buf
        });

        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut err) = stderr_pipe {
                let _ = err.read_to_end(&mut buf).await;
            }
            buf
        });

        let status = match timeout(self.options.process_timeout, child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(err)) => return Err(DpcError::Io(err)),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(DpcError::Config(format!(
                    "Playwright timed out after {:?}",
                    self.options.process_timeout
                )));
            }
        };

        let stdout = stdout_task.await.unwrap_or_else(|_| Vec::new());
        let stderr = stderr_task.await.unwrap_or_else(|_| Vec::new());

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            return Err(map_playwright_error(status.to_string(), &stderr));
        }

        let stdout = String::from_utf8_lossy(&stdout);
        if let Ok(payload) = serde_json::from_str::<ScriptResult>(&stdout) {
            if payload.status != "ok" {
                let detail = payload
                    .message
                    .as_deref()
                    .unwrap_or("no additional details");
                return Err(DpcError::Config(format!(
                    "Playwright returned non-ok status {}: {}",
                    payload.status, detail
                )));
            }
        } else {
            return Err(DpcError::Config(format!(
                "Unexpected Playwright output: {}",
                stdout.trim()
            )));
        }

        Ok(PageRenderResult {
            screenshot_path: screenshot_path.map(|path| path.to_path_buf()),
            viewport: self.options.viewport,
            elapsed: start.elapsed(),
        })
    }

    async fn ensure_node_available(&self) -> Result<()> {
        let mut cmd = Command::new(&self.options.node_command);
        cmd.arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let status = tokio::time::timeout(NODE_CHECK_TIMEOUT, cmd.status())
            .await
            .map_err(|_| {
                DpcError::Config(format!(
                    "Timed out checking node availability after {:?}",
                    NODE_CHECK_TIMEOUT
                ))
            })?
            .map_err(|err| map_spawn_error(err, &self.options.node_command))?;

        if !status.success() {
            return Err(DpcError::Config(format!(
                "Node command {:?} is not available (exit {})",
                self.options.node_command, status
            )));
        }

        Ok(())
    }
}

#[derive(Debug, serde::Deserialize)]
struct ScriptResult {
    status: String,
    message: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ScriptError {
    status: String,
    message: String,
}

fn map_spawn_error(err: io::Error, command: &str) -> DpcError {
    if err.kind() == io::ErrorKind::NotFound {
        DpcError::Config(format!(
            "Unable to spawn Playwright helper; '{}' was not found on PATH",
            command
        ))
    } else {
        DpcError::Io(err)
    }
}

fn map_playwright_error(status_text: impl Into<String>, stderr: &str) -> DpcError {
    if let Ok(error) = serde_json::from_str::<ScriptError>(stderr) {
        let message = if error.message.contains("Cannot find module 'playwright'") {
            "Playwright npm package is missing; install with `npm install playwright`.".to_string()
        } else {
            format!(
                "Playwright error (status {}): {}",
                error.status, error.message
            )
        };
        return DpcError::Config(message);
    }

    DpcError::Config(format!(
        "Playwright exited with status {}: {}",
        status_text.into(),
        stderr.trim()
    ))
}

/// Options for URL to NormalizedView conversion.
#[derive(Debug, Clone)]
pub struct UrlToViewOptions {
    pub node_command: String,
    pub viewport: Viewport,
    pub headless: bool,
    pub navigation_timeout: Duration,
    pub network_idle_timeout: Duration,
    pub process_timeout: Duration,
}

impl Default for UrlToViewOptions {
    fn default() -> Self {
        Self {
            node_command: "node".to_string(),
            viewport: Viewport::default(),
            headless: true,
            navigation_timeout: DEFAULT_NAVIGATION_TIMEOUT,
            network_idle_timeout: DEFAULT_NETWORK_IDLE_TIMEOUT,
            process_timeout: DEFAULT_PROCESS_TIMEOUT,
        }
    }
}

impl From<BrowserOptions> for UrlToViewOptions {
    fn from(opts: BrowserOptions) -> Self {
        Self {
            node_command: opts.node_command,
            viewport: opts.viewport,
            headless: opts.headless,
            navigation_timeout: opts.navigation_timeout,
            network_idle_timeout: opts.network_idle_timeout,
            process_timeout: opts.process_timeout,
        }
    }
}

/// Converts a URL to a NormalizedView by rendering it in a headless browser.
///
/// This function:
/// - Navigates to the URL and waits for network idle
/// - Captures a screenshot at the specified viewport
/// - Extracts the DOM tree with bounding boxes and computed styles
///
/// # Arguments
/// * `url` - The URL to render
/// * `screenshot_path` - Path where the screenshot should be saved
/// * `options` - Configuration options for the browser
///
/// # Returns
/// A NormalizedView containing the screenshot path, viewport dimensions, and DOM snapshot.
pub async fn url_to_normalized_view(
    url: &str,
    screenshot_path: &Path,
    options: UrlToViewOptions,
) -> Result<NormalizedView> {
    let mut cmd = Command::new(&options.node_command);
    cmd.arg("-e")
        .arg(PLAYWRIGHT_SCRIPT_WITH_DOM)
        .arg(url)
        .arg(options.viewport.width.to_string())
        .arg(options.viewport.height.to_string())
        .arg(options.navigation_timeout.as_millis().to_string())
        .arg(options.network_idle_timeout.as_millis().to_string())
        .arg(screenshot_path.to_string_lossy().to_string())
        .arg(if options.headless { "1" } else { "0" })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|err| map_spawn_error(err, &options.node_command))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut out) = stdout_pipe {
            let _ = out.read_to_end(&mut buf).await;
        }
        buf
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr_pipe {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    });

    let status = match timeout(options.process_timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(DpcError::Io(err)),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(DpcError::Config(format!(
                "Playwright timed out after {:?}",
                options.process_timeout
            )));
        }
    };

    let stdout = stdout_task.await.unwrap_or_else(|_| Vec::new());
    let stderr = stderr_task.await.unwrap_or_else(|_| Vec::new());

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        return Err(map_playwright_error(status.to_string(), &stderr));
    }

    let stdout = String::from_utf8_lossy(&stdout);
    let result: ScriptResultWithDom = serde_json::from_str(&stdout).map_err(|e| {
        DpcError::Config(format!(
            "Failed to parse Playwright output: {} - raw: {}",
            e,
            stdout.trim()
        ))
    })?;

    if result.status != "ok" {
        return Err(DpcError::Config(format!(
            "Playwright returned non-ok status: {}",
            result.status
        )));
    }

    let dom_data = result.dom.ok_or_else(|| {
        DpcError::Config("Playwright returned ok status but no DOM data".to_string())
    })?;

    let dom_snapshot = convert_raw_dom(dom_data);

    Ok(NormalizedView {
        kind: ResourceKind::Url,
        screenshot_path: screenshot_path.to_path_buf(),
        width: options.viewport.width,
        height: options.viewport.height,
        dom: Some(dom_snapshot),
        figma_tree: None,
        ocr_blocks: None,
    })
}

// Raw types for deserializing Playwright output
#[derive(Debug, serde::Deserialize)]
struct ScriptResultWithDom {
    status: String,
    dom: Option<RawDomSnapshot>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDomSnapshot {
    url: Option<String>,
    title: Option<String>,
    #[serde(default)]
    nodes: Vec<RawDomNode>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDomNode {
    id: String,
    tag: String,
    #[serde(default)]
    children: Vec<String>,
    parent: Option<String>,
    #[serde(default)]
    attributes: HashMap<String, String>,
    text: Option<String>,
    bounding_box: RawBoundingBox,
    computed_style: Option<RawComputedStyle>,
}

#[derive(Debug, serde::Deserialize)]
struct RawBoundingBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComputedStyle {
    font_family: Option<String>,
    font_size: Option<f32>,
    font_weight: Option<String>,
    line_height: Option<f32>,
    color: Option<String>,
    background_color: Option<String>,
    display: Option<String>,
    visibility: Option<String>,
    opacity: Option<f32>,
}

fn convert_raw_dom(dom_data: RawDomSnapshot) -> DomSnapshot {
    let nodes: Vec<DomNode> = dom_data
        .nodes
        .into_iter()
        .map(|raw| DomNode {
            id: raw.id,
            tag: raw.tag,
            children: raw.children,
            parent: raw.parent,
            attributes: raw.attributes,
            text: raw.text,
            bounding_box: BoundingBox {
                x: raw.bounding_box.x,
                y: raw.bounding_box.y,
                width: raw.bounding_box.width,
                height: raw.bounding_box.height,
            },
            computed_style: raw.computed_style.map(|s| ComputedStyle {
                font_family: s.font_family,
                font_size: s.font_size,
                font_weight: s.font_weight,
                line_height: s.line_height,
                color: s.color,
                background_color: s.background_color,
                display: s.display,
                visibility: s.visibility,
                opacity: s.opacity,
            }),
        })
        .collect();

    DomSnapshot {
        url: dom_data.url,
        title: dom_data.title,
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn browser_options_default_values() {
        let opts = BrowserOptions::default();
        assert_eq!(opts.node_command, "node");
        assert!(opts.headless);
        assert_eq!(opts.max_concurrent_sessions, 1);
        assert_eq!(opts.viewport.width, 1440);
        assert_eq!(opts.viewport.height, 900);
        assert_eq!(opts.navigation_timeout, DEFAULT_NAVIGATION_TIMEOUT);
        assert_eq!(opts.network_idle_timeout, DEFAULT_NETWORK_IDLE_TIMEOUT);
        assert_eq!(opts.process_timeout, DEFAULT_PROCESS_TIMEOUT);
    }

    #[test]
    fn semaphore_never_zero() {
        let manager = BrowserManager::new(BrowserOptions {
            max_concurrent_sessions: 0,
            ..BrowserOptions::default()
        });

        assert_eq!(manager.semaphore.available_permits(), 1);
    }

    #[test]
    fn url_to_view_options_default_values() {
        let opts = UrlToViewOptions::default();
        assert_eq!(opts.node_command, "node");
        assert!(opts.headless);
        assert_eq!(opts.viewport.width, 1440);
        assert_eq!(opts.viewport.height, 900);
        assert_eq!(opts.navigation_timeout, DEFAULT_NAVIGATION_TIMEOUT);
        assert_eq!(opts.network_idle_timeout, DEFAULT_NETWORK_IDLE_TIMEOUT);
        assert_eq!(opts.process_timeout, DEFAULT_PROCESS_TIMEOUT);
    }

    #[test]
    fn url_to_view_options_from_browser_options() {
        let browser_opts = BrowserOptions {
            node_command: "custom-node".to_string(),
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            headless: false,
            navigation_timeout: Duration::from_secs(30),
            network_idle_timeout: Duration::from_secs(10),
            process_timeout: Duration::from_secs(60),
            max_concurrent_sessions: 4,
        };

        let view_opts: UrlToViewOptions = browser_opts.into();
        assert_eq!(view_opts.node_command, "custom-node");
        assert!(!view_opts.headless);
        assert_eq!(view_opts.viewport.width, 1920);
        assert_eq!(view_opts.viewport.height, 1080);
        assert_eq!(view_opts.navigation_timeout, Duration::from_secs(30));
        assert_eq!(view_opts.network_idle_timeout, Duration::from_secs(10));
        assert_eq!(view_opts.process_timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn ensure_node_available_fails_for_missing_binary() {
        let manager = BrowserManager::new(BrowserOptions {
            node_command: "definitely-not-a-binary".to_string(),
            ..BrowserOptions::default()
        });

        let result = manager.ensure_node_available().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn render_url_to_normalized_view_checks_node() {
        let manager = BrowserManager::new(BrowserOptions {
            node_command: "definitely-not-a-binary".to_string(),
            ..BrowserOptions::default()
        });

        let result = manager
            .render_url_to_normalized_view("https://example.com", Path::new("tmp.png"))
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn raw_dom_snapshot_deserializes_correctly() {
        let json = r#"{
            "url": "https://example.com",
            "title": "Example Page",
            "nodes": [{
                "id": "node-0",
                "tag": "div",
                "children": ["node-1"],
                "parent": null,
                "attributes": {"class": "container"},
                "text": "Hello",
                "boundingBox": {"x": 0, "y": 0, "width": 100, "height": 50},
                "computedStyle": {
                    "fontFamily": "Arial",
                    "fontSize": 16.0,
                    "fontWeight": "400",
                    "lineHeight": 24.0,
                    "color": "rgb(0, 0, 0)",
                    "backgroundColor": "rgb(255, 255, 255)",
                    "display": "block",
                    "visibility": "visible",
                    "opacity": 0.5
                }
            }]
        }"#;

        let snapshot: RawDomSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.url, Some("https://example.com".to_string()));
        assert_eq!(snapshot.title, Some("Example Page".to_string()));
        assert_eq!(snapshot.nodes.len(), 1);

        let node = &snapshot.nodes[0];
        assert_eq!(node.id, "node-0");
        assert_eq!(node.tag, "div");
        assert_eq!(node.children, vec!["node-1"]);
        assert!(node.parent.is_none());
        assert_eq!(node.attributes.get("class"), Some(&"container".to_string()));
        assert_eq!(node.text, Some("Hello".to_string()));
        assert_eq!(node.bounding_box.width, 100.0);

        let style = node.computed_style.as_ref().unwrap();
        assert_eq!(style.font_family, Some("Arial".to_string()));
        assert_eq!(style.font_size, Some(16.0));
        assert_eq!(style.display.as_deref(), Some("block"));
        assert_eq!(style.visibility.as_deref(), Some("visible"));
        assert_eq!(style.opacity, Some(0.5));
    }

    #[test]
    fn script_result_with_dom_deserializes() {
        let json = r#"{
            "status": "ok",
            "dom": {
                "url": "https://test.com",
                "title": "Test",
                "nodes": []
            }
        }"#;

        let result: ScriptResultWithDom = serde_json::from_str(json).unwrap();
        assert_eq!(result.status, "ok");
        assert!(result.dom.is_some());
        let dom = result.dom.unwrap();
        assert_eq!(dom.url, Some("https://test.com".to_string()));
        assert!(dom.nodes.is_empty());
    }

    #[test]
    fn convert_raw_dom_copies_style_fields() {
        let raw = RawDomSnapshot {
            url: Some("https://example.com".into()),
            title: Some("Example".into()),
            nodes: vec![RawDomNode {
                id: "n1".into(),
                tag: "div".into(),
                children: vec![],
                parent: None,
                attributes: HashMap::new(),
                text: Some("hello".into()),
                bounding_box: RawBoundingBox {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0,
                    height: 4.0,
                },
                computed_style: Some(RawComputedStyle {
                    font_family: Some("Arial".into()),
                    font_size: Some(12.0),
                    font_weight: Some("700".into()),
                    line_height: Some(16.0),
                    color: Some("rgb(0,0,0)".into()),
                    background_color: Some("rgb(255,255,255)".into()),
                    display: Some("block".into()),
                    visibility: Some("visible".into()),
                    opacity: Some(0.8),
                }),
            }],
        };

        let snapshot = convert_raw_dom(raw);
        let node = snapshot.nodes.first().unwrap();

        assert_eq!(node.text.as_deref(), Some("hello"));
        let style = node.computed_style.as_ref().unwrap();
        assert_eq!(style.font_family.as_deref(), Some("Arial"));
        assert_eq!(style.display.as_deref(), Some("block"));
        assert_eq!(style.visibility.as_deref(), Some("visible"));
        assert_eq!(style.opacity, Some(0.8));
    }

    #[test]
    fn script_error_reports_missing_playwright_module() {
        let err = map_playwright_error(
            "exit status: 1",
            r#"{"status":"error","message":"Cannot find module 'playwright'"}"#,
        );
        let msg = format!("{}", err);
        assert!(msg.contains("Playwright npm package is missing"));
    }

    #[test]
    fn script_error_preserves_other_messages() {
        let err = map_playwright_error(
            "exit status: 1",
            r#"{"status":"error","message":"Timeout navigating to https://example.com"}"#,
        );
        let msg = format!("{}", err);
        assert!(msg.contains("Playwright error"));
        assert!(msg.contains("Timeout navigating"));
    }
}
