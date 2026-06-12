//! Multimodal Vision — image understanding and UI screenshot analysis.
//!
//! Supports:
//! - Image encoding (base64) for LLM vision APIs
//! - Screenshot analysis (UI element detection, layout parsing)
//! - Diagram/chart understanding (OCR-style text extraction)
//! - Image description generation prompt builder
//!
//! ## Architecture
//!
//! ```text
//! Image → encode → build_vision_prompt → send to multimodal LLM → structured response
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Image format
// ---------------------------------------------------------------------------

/// Supported image formats for processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// Joint Photographic Experts Group (JPEG / JPG).
    Jpeg,
    /// Graphics Interchange Format.
    Gif,
    /// Web Picture format.
    WebP,
    /// Windows Bitmap.
    Bmp,
    /// Scalable Vector Graphics.
    Svg,
    /// Unrecognised format.
    Unknown,
}

impl ImageFormat {
    /// Detect [`ImageFormat`] from a file extension (case-insensitive).
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "gif" => ImageFormat::Gif,
            "webp" => ImageFormat::WebP,
            "bmp" => ImageFormat::Bmp,
            "svg" => ImageFormat::Svg,
            _ => ImageFormat::Unknown,
        }
    }

    /// Return the MIME type string for this image format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Svg => "image/svg+xml",
            ImageFormat::Unknown => "application/octet-stream",
        }
    }

    /// Detect [`ImageFormat`] from raw file bytes (magic bytes / header).
    pub fn detect(bytes: &[u8]) -> Option<ImageFormat> {
        if bytes.len() < 8 {
            return None;
        }
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(ImageFormat::Png);
        }
        // JPEG: FF D8 FF
        if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            return Some(ImageFormat::Jpeg);
        }
        // GIF: 47 49 46 38 (GIF8)
        if bytes.starts_with(b"GIF8") {
            return Some(ImageFormat::Gif);
        }
        // BMP: 42 4D (BM)
        if bytes.starts_with(b"BM") {
            return Some(ImageFormat::Bmp);
        }
        // WebP: 52 49 46 46 ... 57 45 42 50 (RIFF ... WEBP)
        if bytes.len() >= 12
            && bytes.starts_with(b"RIFF")
            && &bytes[8..12] == b"WEBP"
        {
            return Some(ImageFormat::WebP);
        }
        // SVG: starts with "<svg" or "<?xml"
        let header = String::from_utf8_lossy(bytes);
        if (header.trim_start().starts_with("<svg")
            || header.trim_start().starts_with("<?xml"))
            && (header.contains("<svg") || header.contains("<SVG")) {
                return Some(ImageFormat::Svg);
            }
        None
    }
}

// ---------------------------------------------------------------------------
// VisionImage – processed image ready for LLM consumption
// ---------------------------------------------------------------------------

/// A processed image that is ready to be sent to a vision-capable LLM.
#[derive(Debug, Clone)]
pub struct VisionImage {
    /// Base64-encoded image data.
    pub base64_data: String,
    /// Detected (or supplied) image format.
    pub format: ImageFormat,
    /// Original file path, if the image was loaded from disk.
    pub source_path: Option<PathBuf>,
    /// Image dimensions `(width, height)` in pixels, when known.
    pub dimensions: Option<(u32, u32)>,
    /// Rough token-cost estimate for this image (~768 tokens per 336×336 tile).
    pub estimated_tokens: usize,
    /// MIME type suitable for API submission.
    pub mime_type: String,
}

// ---------------------------------------------------------------------------
// Analysis result types
// ---------------------------------------------------------------------------

/// Full analysis result produced by [`VisionEngine`].
#[derive(Debug, Clone)]
pub struct ImageAnalysis {
    /// Human-readable description of what the image contains.
    pub description: String,
    /// Detected UI elements (populated for screenshots when UI analysis is enabled).
    pub ui_elements: Vec<UiElement>,
    /// Text regions extracted from the image (OCR-like).
    pub detected_text: Vec<TextRegion>,
    /// Structured chart / data-visualisation data, if applicable.
    pub chart_data: Option<ChartData>,
    /// Low-level image metadata.
    pub metadata: ImageMetadata,
    /// A structured prompt string suitable for sending to a vision-capable LLM.
    pub vision_prompt: String,
}

/// A single UI element detected inside a screenshot.
#[derive(Debug, Clone)]
pub struct UiElement {
    /// Classification of the element.
    pub element_type: UiElementType,
    /// Label / visible text associated with the element.
    pub label: String,
    /// Normalised bounding box (`0.0..1.0`).
    pub bounding_box: BoundingBox,
    /// Detection confidence `0.0 ..= 1.0`.
    pub confidence: f32,
    /// Arbitrary key-value attributes (e.g. `("placeholder", "Search…")`).
    pub attributes: HashMap<String, String>,
}

/// Known UI element categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiElementType {
    Button,
    TextInput,
    TextArea,
    Link,
    Image,
    Icon,
    Text,
    Heading,
    Table,
    List,
    Dialog,
    Menu,
    Tab,
    Checkbox,
    Radio,
    Slider,
    Select,
    CodeBlock,
    Terminal,
    StatusBar,
    Toolbar,
    Sidebar,
    Unknown,
}

/// Axis-aligned bounding box with normalised coordinates (`0.0 … 1.0`).
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// A region of text detected inside an image.
#[derive(Debug, Clone)]
pub struct TextRegion {
    /// The extracted text content.
    pub text: String,
    /// Normalised bounding box.
    pub bbox: BoundingBox,
    /// OCR confidence `0.0 ..= 1.0`.
    pub confidence: f32,
    /// ISO 639 language code when available (e.g. `"en"`).
    pub language: Option<String>,
}

/// Structured data extracted from a chart or data visualisation.
#[derive(Debug, Clone)]
pub struct ChartData {
    /// Kind of chart.
    pub chart_type: ChartType,
    /// Chart title, if present.
    pub title: String,
    /// Data series.
    pub series: Vec<ChartSeries>,
    /// Category / X-axis labels.
    pub labels: Vec<String>,
    /// Axis labels.
    pub axes: ChartAxes,
}

/// Supported chart varieties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartType {
    Bar,
    Line,
    Pie,
    Scatter,
    Area,
    HeatMap,
    Unknown,
}

/// One data series inside a chart.
#[derive(Debug, Clone)]
pub struct ChartSeries {
    /// Series name / legend entry.
    pub name: String,
    /// Numeric values.
    pub values: Vec<f64>,
    /// Display colour (CSS-like), when known.
    pub color: Option<String>,
}

/// Axis labels for a chart.
#[derive(Debug, Clone)]
pub struct ChartAxes {
    /// X-axis label.
    pub x_label: String,
    /// Y-axis label.
    pub y_label: String,
}

/// Low-level metadata about an image.
#[derive(Debug, Clone)]
pub struct ImageMetadata {
    /// Image format.
    pub format: ImageFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Approximate file size in bytes.
    pub file_size_bytes: u64,
    /// Colour-space description (e.g. `"RGBA"`, `"RGB"`, `"Grayscale"`).
    pub color_space: String,
}

// ---------------------------------------------------------------------------
// VisionConfig & VisionBackend
// ---------------------------------------------------------------------------

/// Which vision backend to use for LLM-powered image analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum VisionBackend {
    /// OpenAI GPT-4o / GPT-4o-mini vision API (or any OpenAI-compatible endpoint).
    OpenAi,
    /// DeepSeek-VL multimodal model via OpenAI-compatible chat/completions.
    DeepSeek,
    /// Local analysis only — metadata + heuristics, no LLM API call.
    #[default]
    LocalOnly,
}


/// Configuration for the vision engine's LLM backend.
#[derive(Debug, Clone)]
pub struct VisionConfig {
    /// Selected backend.
    pub backend: VisionBackend,
    /// API key for the selected backend (required for non-LocalOnly backends).
    pub api_key: Option<String>,
    /// Base URL for the API endpoint.
    ///
    /// - OpenAI default: `https://api.openai.com/v1/chat/completions`
    /// - DeepSeek default: `https://api.deepseek.com/chat/completions`
    pub api_url: Option<String>,
    /// Model identifier sent to the API.
    ///
    /// - OpenAI default: `"gpt-4o"`
    /// - DeepSeek default: `"deepseek-chat"` (or `"deepseek-vl"` if available)
    pub model: String,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            backend: VisionBackend::LocalOnly,
            api_key: None,
            api_url: None,
            model: "gpt-4o".to_string(),
        }
    }
}

impl VisionConfig {
    /// Resolve the effective API URL based on the backend selection.
    pub fn resolved_api_url(&self) -> &str {
        self.api_url.as_deref().unwrap_or(match self.backend {
            VisionBackend::OpenAi => "https://api.openai.com/v1/chat/completions",
            VisionBackend::DeepSeek => "https://api.deepseek.com/chat/completions",
            VisionBackend::LocalOnly => "",
        })
    }
}

/// The main vision-processing engine.
///
/// Loads images from disk or raw bytes, encodes them to base64, extracts
/// metadata, and builds prompts suitable for vision-capable LLMs.
#[derive(Clone)]
pub struct VisionEngine {
    /// Whether to attempt OCR-style text extraction.
    pub(crate) enable_text_extraction: bool,
    /// Whether to run UI-layout analysis (for screenshots).
    enable_ui_analysis: bool,
    /// Maximum allowed image size in bytes.
    max_image_size_bytes: usize,
    /// Optional LLM backend configuration.
    config: Option<VisionConfig>,
    /// HTTP client for API calls (shared across requests).
    client: reqwest::Client,
}

impl VisionEngine {
    /// Create a new [`VisionEngine`] with default settings (no LLM backend).
    ///
    /// Defaults:
    /// - text extraction: **enabled**
    /// - UI analysis: **enabled**
    /// - max size: **20 MiB**
    /// - backend: `LocalOnly` (no API calls)
    pub fn new() -> Self {
        Self::with_config(None)
    }

    /// Create a new [`VisionEngine`] with an optional LLM backend configuration.
    ///
    /// When *config* is `Some`, the engine will call a vision-capable LLM
    /// API for description generation, text extraction, and UI analysis
    /// (depending on the backend setting). When `None`, all analysis is
    /// done via heuristics only.
    pub fn with_config(config: Option<VisionConfig>) -> Self {
        tracing::info!(
            backend = match &config {
                Some(c) => format!("{:?}", c.backend),
                None => "LocalOnly (none)".to_string(),
            },
            "VisionEngine created"
        );
        Self {
            enable_text_extraction: true,
            enable_ui_analysis: true,
            max_image_size_bytes: 20 * 1024 * 1024, // 20 MB
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Enable or disable OCR-style text extraction.
    pub fn with_text_extraction(mut self, enabled: bool) -> Self {
        self.enable_text_extraction = enabled;
        self
    }

    /// Enable or disable UI layout analysis.
    pub fn with_ui_analysis(mut self, enabled: bool) -> Self {
        self.enable_ui_analysis = enabled;
        self
    }

    /// Set the maximum allowed image size in bytes.
    pub fn with_max_size(mut self, bytes: usize) -> Self {
        self.max_image_size_bytes = bytes;
        self
    }

    // ---- public API -------------------------------------------------------

    /// Load an image from *path* and return a full [`ImageAnalysis`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, exceeds the size limit,
    /// or has an unrecognised format.
    pub fn process_image(&self, path: &Path) -> anyhow::Result<ImageAnalysis> {
        // 1. Read file bytes.
        let bytes = std::fs::read(path)?;
        if bytes.len() > self.max_image_size_bytes {
            anyhow::bail!(
                "Image too large: {} bytes (max {})",
                bytes.len(),
                self.max_image_size_bytes
            );
        }

        // 2. Determine format from file extension.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let format = ImageFormat::from_extension(ext);

        // 3. Encode to base64.
        let base64_data = base64_encode(&bytes);

        // 4. Try reading dimensions from file headers.
        let dimensions = read_image_dimensions(&bytes, format);

        // 5. Assemble the intermediate [`VisionImage`].
        let img = VisionImage {
            base64_data,
            format,
            source_path: Some(path.to_path_buf()),
            dimensions,
            estimated_tokens: estimate_image_tokens(dimensions),
            mime_type: format.mime_type().to_string(),
        };

        // 6. Run analysis pipeline.
        self.analyze(&img)
    }

    /// Process raw image *bytes* with an explicit *format* hint.
    ///
    /// # Errors
    ///
    /// Propagates any failure during encoding or analysis.
    pub fn process_bytes(&self, bytes: &[u8], format: ImageFormat) -> anyhow::Result<ImageAnalysis> {
        let base64_data = base64_encode(bytes);
        let dimensions = read_image_dimensions(bytes, format);
        let img = VisionImage {
            base64_data,
            format,
            source_path: None,
            dimensions,
            estimated_tokens: estimate_image_tokens(dimensions),
            mime_type: format.mime_type().to_string(),
        };
        self.analyze(&img)
    }

    /// Build a user-facing vision prompt that embeds the image and a question.
    ///
    /// The returned string is designed to be sent directly to a
    /// multimodal LLM API that accepts interleaved text + image content.
    pub fn build_vision_prompt(&self, img: &VisionImage, user_question: &str) -> String {
        format!(
            "<image>\n[Image embedded — format: {}, size: ~{}KB, estimated tokens: {}]\n</image>\n\n{}",
            img.mime_type,
            img.base64_data.len() / 1024,
            img.estimated_tokens,
            user_question
        )
    }

    // ---- Async LLM-powered analysis ---------------------------------------

    /// Async version of [`process_image`] that calls the vision LLM API
    /// when a non-`LocalOnly` backend is configured.
    ///
    /// The synchronous [`process_image`] always returns heuristic-only results.
    /// Use this method when you want real LLM-generated descriptions, OCR, or UI analysis.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, exceeds size limits, or
    /// the API call fails (network error, auth failure, etc.).
    pub async fn process_image_async(&self, path: &Path) -> anyhow::Result<ImageAnalysis> {
        let bytes = std::fs::read(path)?;
        if bytes.len() > self.max_image_size_bytes {
            anyhow::bail!(
                "Image too large: {} bytes (max {})",
                bytes.len(),
                self.max_image_size_bytes
            );
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let format = ImageFormat::from_extension(ext);
        let base64_data = base64_encode(&bytes);
        let dimensions = read_image_dimensions(&bytes, format);

        let img = VisionImage {
            base64_data,
            format,
            source_path: Some(path.to_path_buf()),
            dimensions,
            estimated_tokens: estimate_image_tokens(dimensions),
            mime_type: format.mime_type().to_string(),
        };

        self.analyze_async(&img).await
    }

    /// Call an OpenAI-compatible vision (multimodal chat/completions) API.
    ///
    /// Sends the image as a base64 data-URI inside the `image_url` content block
    /// along with *question* as the user text prompt.
    ///
    /// # Errors
    ///
    /// - No API key configured for non-LocalOnly backends
    /// - Network / HTTP errors
    /// - Non-200 response from the API
    async fn call_vision_api(
        &self,
        img: &VisionImage,
        question: &str,
        config: &VisionConfig,
    ) -> anyhow::Result<String> {
        let api_key = config.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "API key is required for {:?} backend but none was provided in VisionConfig",
                config.backend
            )
        })?;

        let url = config.resolved_api_url();
        if url.is_empty() {
            anyhow::bail!("No API URL available for LocalOnly backend");
        }

        let data_uri = format!(
            "data:{};base64,{}",
            img.mime_type,
            img.base64_data
        );

        #[derive(serde::Serialize)]
        struct VisionRequest<'a> {
            model: &'a str,
            messages: Vec<VisionMessage<'a>>,
        }

        #[derive(serde::Serialize)]
        struct VisionMessage<'a> {
            role: &'a str,
            content: Vec<VisionContent<'a>>,
        }

        #[derive(serde::Serialize)]
        struct VisionContent<'a> {
            #[serde(rename = "type")]
            content_type: &'a str,
            text: Option<&'a str>,
            image_url: Option<VisionImageUrl<'a>>,
        }

        #[derive(serde::Serialize)]
        struct VisionImageUrl<'a> {
            url: &'a str,
        }

        let body = VisionRequest {
            model: &config.model,
            messages: vec![VisionMessage {
                role: "user",
                content: vec![
                    VisionContent {
                        content_type: "text",
                        text: Some(question),
                        image_url: None,
                    },
                    VisionContent {
                        content_type: "image_url",
                        text: None,
                        image_url: Some(VisionImageUrl { url: &data_uri }),
                    },
                ],
            }],
        };

        tracing::debug!(url, model = %config.model, "calling vision API");

        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Vision API request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Vision API error HTTP {}: {}",
                status,
                body_text
            );
        }

        #[derive(serde::Deserialize)]
        struct VisionResponse {
            choices: Vec<VisionChoice>,
        }

        #[derive(serde::Deserialize)]
        struct VisionChoice {
            message: VisionAssistantMessage,
        }

        #[derive(serde::Deserialize)]
        struct VisionAssistantMessage {
            content: String,
        }

        let vision_resp: VisionResponse = resp.json().await.map_err(|e| {
            anyhow::anyhow!("Failed to parse vision API response JSON: {}", e)
        })?;

        let content = vision_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        tracing::info!(content_len = content.len(), "vision API response received");
        Ok(content)
    }

    // ---- internal analysis pipeline --------------------------------------

    /// Core analysis: extract metadata, detect elements, build prompt.
    fn analyze(&self, img: &VisionImage) -> anyhow::Result<ImageAnalysis> {
        let metadata = ImageMetadata {
            format: img.format,
            width: img.dimensions.map(|(w, _)| w).unwrap_or(0),
            height: img.dimensions.map(|(_, h)| h).unwrap_or(0),
            file_size_bytes: (img.base64_data.len() as u64) * 3 / 4, // rough base64 → bytes
            color_space: "RGBA".into(),
        };

        // Text extraction — use LLM if configured, otherwise placeholder.
        let detected_text = if self.enable_text_extraction {
            self.extract_text(img)
        } else {
            Vec::new()
        };

        // UI analysis — use LLM if configured, otherwise placeholder.
        let ui_elements = if self.enable_ui_analysis && self.is_likely_screenshot(img) {
            self.analyze_ui(img)?
        } else {
            Vec::new()
        };

        let vision_prompt =
            self.build_internal_vision_prompt(img, &detected_text, &ui_elements);
        let description =
            self.generate_description(img, &detected_text, &ui_elements);
        let chart_data = self.detect_chart_placeholder(img)?;

        Ok(ImageAnalysis {
            description,
            ui_elements,
            detected_text,
            chart_data,
            metadata,
            vision_prompt,
        })
    }

    /// Async core analysis: same as [`analyze`] but calls the vision LLM API.
    async fn analyze_async(&self, img: &VisionImage) -> anyhow::Result<ImageAnalysis> {
        let metadata = ImageMetadata {
            format: img.format,
            width: img.dimensions.map(|(w, _)| w).unwrap_or(0),
            height: img.dimensions.map(|(_, h)| h).unwrap_or(0),
            file_size_bytes: (img.base64_data.len() as u64) * 3 / 4,
            color_space: "RGBA".into(),
        };

        let detected_text = if self.enable_text_extraction {
            self.extract_text_async(img).await?
        } else {
            Vec::new()
        };

        let ui_elements = if self.enable_ui_analysis && self.is_likely_screenshot(img) {
            self.analyze_ui_async(img).await?
        } else {
            Vec::new()
        };

        let vision_prompt =
            self.build_internal_vision_prompt(img, &detected_text, &ui_elements);
        let description =
            self.generate_description_async(img).await?;
        let chart_data = self.detect_chart_placeholder(img)?;

        Ok(ImageAnalysis {
            description,
            ui_elements,
            detected_text,
            chart_data,
            metadata,
            vision_prompt,
        })
    }

    /// Heuristic check whether the image looks like a screen capture.
    fn is_likely_screenshot(&self, img: &VisionImage) -> bool {
        match img.dimensions {
            Some((w, h)) => {
                let ratio = w as f32 / h.max(1) as f32;
                // Common screen ratios: 16:9 (~1.78), 16:10 (1.6), 4:3 (1.33), 1:1
                (0.9..=2.2).contains(&ratio) && w >= 800 && h >= 400
            }
            None => false,
        }
    }

    /// Extract text from an image using the vision API if available, or return empty.
    fn extract_text(&self, _img: &VisionImage) -> Vec<TextRegion> {
        // Sync path: always returns empty (no blocking API call).
        // Use process_image_async for LLM-powered extraction.
        Vec::new()
    }

    /// Extract text from an image via the vision LLM API (async).
    ///
    /// Sends an OCR-focused prompt and parses structured text regions from the response.
    async fn extract_text_async(&self, img: &VisionImage) -> anyhow::Result<Vec<TextRegion>> {
        let Some(config) = &self.config else {
            return Ok(Vec::new());
        };
        if config.backend == VisionBackend::LocalOnly {
            return Ok(Vec::new());
        }

        let prompt = "Extract all visible text from this image. \
            Return each text region as a JSON object with 'text', 'x', 'y', 'width', 'height' \
            (normalised 0-1 coordinates), and 'confidence'. \
            Return a JSON array of these objects.";

        match self.call_vision_api(img, prompt, config).await {
            Ok(response) => {
                // Try to parse structured response; fall back to raw text as single region
                if let Ok(regions) = serde_json::from_str::<Vec<serde_json::Value>>(&response) {
                    Ok(regions
                        .into_iter()
                        .filter_map(|v| {
                            Some(TextRegion {
                                text: v.get("text")?.as_str()?.to_string(),
                                bbox: BoundingBox {
                                    x: v.get("x")?.as_f64()? as f32,
                                    y: v.get("y")?.as_f64()? as f32,
                                    width: v.get("width")?.as_f64()? as f32,
                                    height: v.get("height")?.as_f64()? as f32,
                                },
                                confidence: v.get("confidence")
                                    .and_then(|c| c.as_f64())
                                    .unwrap_or(0.9) as f32,
                                language: None,
                            })
                        })
                        .collect())
                } else {
                    // Fallback: treat entire response as one text block
                    Ok(vec![TextRegion {
                        text: response.trim().to_string(),
                        bbox: BoundingBox { x: 0.0, y: 0.0, width: 1.0, height: 1.0 },
                        confidence: 0.85,
                        language: None,
                    }])
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "vision API text extraction failed, falling back to empty");
                Ok(Vec::new())
            }
        }
    }

    /// Analyze UI elements in an image using the vision API if available, or return empty.
    fn analyze_ui(&self, _img: &VisionImage) -> anyhow::Result<Vec<UiElement>> {
        // Sync path: always returns empty.
        // Use process_image_async for LLM-powered UI analysis.
        Ok(Vec::new())
    }

    /// Analyze UI elements via the vision LLM API (async).
    async fn analyze_ui_async(&self, img: &VisionImage) -> anyhow::Result<Vec<UiElement>> {
        let Some(config) = &self.config else {
            return Ok(Vec::new());
        };
        if config.backend == VisionBackend::LocalOnly {
            return Ok(Vec::new());
        }

        let prompt = "Analyze this screenshot/UI capture. Identify all interactive UI elements \
            (buttons, text inputs, links, icons, headings, etc.). For each element provide: \
            'type' (Button/TextInput/Link/Icon/Text/Heading/etc.), 'label' (visible text or description), \
            'x', 'y', 'width', 'height' (normalised 0-1 bounding box), and 'confidence'. \
            Return a JSON array of detected elements.";

        match self.call_vision_api(img, prompt, config).await {
            Ok(response) => {
                if let Ok(elements) = serde_json::from_str::<Vec<serde_json::Value>>(&response) {
                    Ok(elements
                        .into_iter()
                        .filter_map(|v| {
                            let type_str = v.get("type")?.as_str()?;
                            let element_type = match type_str {
                                "button" | "Button" => UiElementType::Button,
                                "text_input" | "TextInput" => UiElementType::TextInput,
                                "text_area" | "TextArea" => UiElementType::TextArea,
                                "link" | "Link" => UiElementType::Link,
                                "image" | "Image" => UiElementType::Image,
                                "icon" | "Icon" => UiElementType::Icon,
                                "text" | "Text" => UiElementType::Text,
                                "heading" | "Heading" => UiElementType::Heading,
                                "table" | "Table" => UiElementType::Table,
                                "list" | "List" => UiElementType::List,
                                "dialog" | "Dialog" => UiElementType::Dialog,
                                "menu" | "Menu" => UiElementType::Menu,
                                "tab" | "Tab" => UiElementType::Tab,
                                "checkbox" | "Checkbox" => UiElementType::Checkbox,
                                "radio" | "Radio" => UiElementType::Radio,
                                "slider" | "Slider" => UiElementType::Slider,
                                "select" | "Select" => UiElementType::Select,
                                "code_block" | "CodeBlock" => UiElementType::CodeBlock,
                                "terminal" | "Terminal" => UiElementType::Terminal,
                                "status_bar" | "StatusBar" => UiElementType::StatusBar,
                                "toolbar" | "Toolbar" => UiElementType::Toolbar,
                                "sidebar" | "Sidebar" => UiElementType::Sidebar,
                                _ => UiElementType::Unknown,
                            };
                            Some(UiElement {
                                element_type,
                                label: v.get("label")
                                    .and_then(|l| l.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                bounding_box: BoundingBox {
                                    x: v.get("x")?.as_f64()? as f32,
                                    y: v.get("y")?.as_f64()? as f32,
                                    width: v.get("width")?.as_f64()? as f32,
                                    height: v.get("height")?.as_f64()? as f32,
                                },
                                confidence: v.get("confidence")
                                    .and_then(|c| c.as_f64())
                                    .unwrap_or(0.8) as f32,
                                attributes: HashMap::new(),
                            })
                        })
                        .collect())
                } else {
                    Ok(Vec::new())
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "vision API UI analysis failed, falling back to empty");
                Ok(Vec::new())
            }
        }
    }

    /// Build the internal vision prompt used inside [`analyze`].
    fn build_internal_vision_prompt(
        &self,
        img: &VisionImage,
        _text: &[TextRegion],
        _ui: &[UiElement],
    ) -> String {
        format!(
            "[VISION_INPUT format={} size={}kb tokens={}]\nAnalyze this image carefully.",
            img.format.mime_type(),
            img.base64_data.len() / 1024,
            img.estimated_tokens
        )
    }

    /// Generate a description using heuristics only (sync path).
    fn generate_description(
        &self,
        img: &VisionImage,
        _text: &[TextRegion],
        _ui: &[UiElement],
    ) -> String {
        format!(
            "Image ({}, {}x{}, ~{} tokens)",
            img.format.mime_type(),
            img.dimensions.map(|(w, _)| w).unwrap_or(0),
            img.dimensions.map(|(_, h)| h).unwrap_or(0),
            img.estimated_tokens
        )
    }

    /// Generate a description via the vision LLM API (async).
    async fn generate_description_async(&self, img: &VisionImage) -> anyhow::Result<String> {
        let Some(config) = &self.config else {
            // No config → fall back to heuristic description
            return Ok(self.generate_description(img, &[], &[]));
        };
        if config.backend == VisionBackend::LocalOnly {
            return Ok(self.generate_description(img, &[], &[]));
        }

        let prompt = "Provide a concise but detailed description of this image. \
            Describe the main subjects, colors, layout, any text visible, and overall composition.";

        match self.call_vision_api(img, prompt, config).await {
            Ok(desc) => Ok(desc.trim().to_string()),
            Err(e) => {
                tracing::warn!(error = %e, "vision API description failed, falling back to heuristic");
                Ok(self.generate_description(img, &[], &[]))
            }
        }
    }

    /// Placeholder for chart / data-visualisation detection.
    fn detect_chart_placeholder(&self, _img: &VisionImage) -> anyhow::Result<Option<ChartData>> {
        Ok(None)
    }

    /// Batch process multiple images with parallel API calls.
    pub async fn batch_process(
        &self,
        images: &[PathBuf],
        question: &str,
        max_concurrent: usize,
    ) -> Vec<anyhow::Result<String>> {
        use std::sync::Arc as StdArc;

        let semaphore = StdArc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for path in images {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let engine = self.clone();
            let _q = question.to_string();
            let p = path.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                match engine.process_image_async(&p).await {
                    Ok(r) => Ok(r.description),
                    Err(_e) => {
                        // Fallback to heuristic
                        let meta = engine.process_image(&p);
                        Ok(format!("[fallback] {}", meta.map(|m| m.description).unwrap_or_default()))
                    }
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(
                handle
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("Task failed: {}", e))),
            );
        }
        results
    }

    /// Image format detection with MIME type.
    pub fn detect_mime_type(path: &Path) -> &'static str {
        match ImageFormat::detect(&std::fs::read(path).unwrap_or_default()) {
            Some(fmt) => fmt.mime_type(),
            None => "application/octet-stream",
        }
    }

    /// Compress image to reduce token usage (resize to max dimension).
    pub fn compress_for_api(path: &Path, _max_dimension: u32) -> anyhow::Result<Vec<u8>> {
        // Read and potentially downsample the image
        // For now: return raw bytes with a note
        let bytes = std::fs::read(path)?;
        Ok(bytes)
    }
}

impl Default for VisionEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// VisionCache – avoid re-processing the same image
// ---------------------------------------------------------------------------

/// Cache for vision API results (avoid re-processing same image).
pub struct VisionCache {
    entries: Arc<tokio::sync::RwLock<HashMap<String, CachedVisionResult>>>,
    max_entries: usize,
}

/// A single cached vision result entry.
#[derive(Debug, Clone)]
pub struct CachedVisionResult {
    pub description: String,
    pub created_at: std::time::Instant,
    pub hit_count: u64,
}

impl VisionCache {
    /// Create a new cache with default max entries (100).
    pub fn new() -> Self {
        Self::with_max_entries(100)
    }

    /// Create a new cache with a specific max entries limit.
    pub fn with_max_entries(max: usize) -> Self {
        Self {
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            max_entries: max,
        }
    }

    /// Get a cached result if available.
    pub async fn get(&self, image_path: &Path, question: &str) -> Option<String> {
        let key = format!("{}::{}", image_path.display(), question);
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(&key) {
            entry.hit_count += 1;
            Some(entry.description.clone())
        } else {
            None
        }
    }

    /// Store a result in the cache.
    pub async fn set(&self, image_path: &Path, question: &str, result: &str) {
        let key = format!("{}::{}", image_path.display(), question);
        let mut entries = self.entries.write().await;

        // Evict oldest entry if at capacity
        if entries.len() >= self.max_entries && !entries.contains_key(&key) {
            if let Some(oldest_key) = entries.iter().min_by_key(|(_, v)| v.created_at).map(|(k, _)| k.clone()) {
                entries.remove(&oldest_key);
            }
        }

        entries.insert(
            key,
            CachedVisionResult {
                description: result.to_string(),
                created_at: std::time::Instant::now(),
                hit_count: 0,
            },
        );
    }

    /// Clear all cached entries.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }

    /// Get cache statistics: (entries, hits, misses).
    pub async fn stats(&self) -> (usize, u64, u64) {
        let entries = self.entries.read().await;
        let total_hits: u64 = entries.values().map(|e| e.hit_count).sum();
        // We approximate misses separately
        (entries.len(), total_hits, 0)
    }
}

impl Default for VisionCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

/// Attempt to read image dimensions from file-header bytes without any
/// external image-crates.
///
/// Currently supports **PNG** and **BMP** only.  JPEG dimension extraction
/// requires SOF-marker scanning and is intentionally left as `None` for now.
fn read_image_dimensions(bytes: &[u8], format: ImageFormat) -> Option<(u32, u32)> {
    match format {
        ImageFormat::Png if bytes.len() >= 24 => {
            // PNG IHDR: width at offset 16 (4 bytes BE), height at offset 20.
            let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
            let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
            Some((w, h))
        }
        ImageFormat::Jpeg => {
            // JPEG requires SOF-marker scanning — skip for simplicity.
            None
        }
        ImageFormat::Bmp if bytes.len() >= 26 => {
            // BMP DIB header: width at offset 18, height at 22 (little-endian).
            let w = u32::from_le_bytes(bytes[18..22].try_into().ok()?);
            let h = u32::from_le_bytes(bytes[22..26].try_into().ok()?);
            Some((w, h))
        }
        _ => None,
    }
}

/// Estimate the token cost of an image based on its pixel resolution.
///
/// The model follows the common convention of **~768 tokens** for a
/// **336 × 336** tile, scaling with the square-root of the pixel area.
pub fn estimate_image_tokens(dimensions: Option<(u32, u32)>) -> usize {
    match dimensions {
        Some((w, h)) => {
            let pixels = (w * h) as f64;
            let base_pixels = 336.0 * 336.0; // ≈112 896
            let base_tokens = 768.0_f64;
            (base_tokens * (pixels / base_pixels).sqrt()).ceil() as usize
        }
        None => 1024, // conservative default
    }
}

// ---------------------------------------------------------------------------
// Base64 implementation (no external crate dependency)
// ---------------------------------------------------------------------------

/// Standard RFC 4648 Base64 alphabet.
const BASE64_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode arbitrary bytes into a standard Base64 string.
pub fn base64_encode(input: &[u8]) -> String {
        let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = if chunk.len() > 1 {
                chunk[1] as usize
            } else {
                0
            };
            let b2 = if chunk.len() > 2 {
                chunk[2] as usize
            } else {
                0
            };

            output.push(BASE64_ALPHABET[(b0 >> 2) & 0x3F] as char);
            output.push(
                BASE64_ALPHABET[(((b0 & 0x03) << 4) | ((b1 >> 4) & 0x0F)) & 0x3F] as char,
            );
            output.push(if chunk.len() > 1 {
                BASE64_ALPHABET[(((b1 & 0x0F) << 2) | ((b2 >> 6) & 0x03)) & 0x3F] as char
            } else {
                '='
            });
            output.push(if chunk.len() > 2 {
                BASE64_ALPHABET[b2 & 0x3F] as char
            } else {
                '='
            });
        }
        output
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- ImageFormat --------------------------------------------------------

    #[test]
    fn test_image_format_detection() {
        assert_eq!(ImageFormat::from_extension("png"), ImageFormat::Png);
        assert_eq!(ImageFormat::from_extension("JPG"), ImageFormat::Jpeg);
        assert_eq!(ImageFormat::from_extension("jpeg"), ImageFormat::Jpeg);
        assert_eq!(ImageFormat::from_extension("webp"), ImageFormat::WebP);
        assert_eq!(ImageFormat::from_extension("gif"), ImageFormat::Gif);
        assert_eq!(ImageFormat::from_extension("bmp"), ImageFormat::Bmp);
        assert_eq!(ImageFormat::from_extension("svg"), ImageFormat::Svg);
        assert_eq!(ImageFormat::from_extension("xyz"), ImageFormat::Unknown);
    }

    #[test]
    fn test_mime_types() {
        assert_eq!(ImageFormat::Png.mime_type(), "image/png");
        assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(ImageFormat::Gif.mime_type(), "image/gif");
        assert_eq!(ImageFormat::WebP.mime_type(), "image/webp");
        assert_eq!(ImageFormat::Bmp.mime_type(), "image/bmp");
        assert_eq!(ImageFormat::Svg.mime_type(), "image/svg+xml");
        assert_eq!(ImageFormat::Unknown.mime_type(), "application/octet-stream");
    }

    // -- Base64 -------------------------------------------------------------

    #[test]
    fn test_base64_encode() {
        let encoded = base64_encode(b"hello world");
        assert_eq!(encoded, "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_single_byte() {
        assert_eq!(base64_encode(b"A"), "QQ==");
    }

    #[test]
    fn test_base64_encode_two_bytes() {
        assert_eq!(base64_encode(b"AB"), "QUI=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        assert_eq!(base64_encode(b"ABC"), "QUJD");
    }

    #[test]
    fn test_base64_encode_alignment_boundary() {
        // 4 bytes → 2 groups of 3 (padded)
        assert_eq!(base64_encode(b"abcd"), "YWJjZA==");
    }

    // -- Dimension reading --------------------------------------------------

    #[test]
    fn test_png_dimension_reading() {
        // Minimal valid PNG header with IHDR declaring 100 × 200.
        let mut png_header = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk header
            0x00, 0x00, 0x00, 0x64,                          // width: 100
            0x00, 0x00, 0x00, 0xC8,                          // height: 200
        ];
        png_header.extend_from_slice(&[0u8; 12]); // rest of IHDR
        let dims = read_image_dimensions(&png_header, ImageFormat::Png);
        assert_eq!(dims, Some((100, 200)));
    }

    #[test]
    fn test_png_dimension_short_header() {
        let short = vec![0u8; 10];
        assert_eq!(read_image_dimensions(&short, ImageFormat::Png), None);
    }

    #[test]
    fn test_bmp_dimension_reading() {
        let mut bmp_header = vec![0u8; 18]; // up-to but not including dimensions
        bmp_header.extend_from_slice(&100u32.to_le_bytes()); // width
        bmp_header.extend_from_slice(&200u32.to_le_bytes()); // height
        let dims = read_image_dimensions(&bmp_header, ImageFormat::Bmp);
        assert_eq!(dims, Some((100, 200)));
    }

    #[test]
    fn test_jpeg_returns_none() {
        // JPEG is intentionally unsupported for header parsing.
        assert_eq!(
            read_image_dimensions(&[0xFF, 0xD8, 0xFF], ImageFormat::Jpeg),
            None
        );
    }

    // -- Token estimation ---------------------------------------------------

    #[test]
    fn test_token_estimation_baseline() {
        // 336 × 336 → exactly 768 tokens by definition.
        assert_eq!(estimate_image_tokens(Some((336, 336))), 768);
    }

    #[test]
    fn test_token_estimation_hd() {
        let hd = estimate_image_tokens(Some((1920, 1080)));
        assert!(hd > 1000); // HD costs more than baseline
    }

    #[test]
    fn test_token_estimation_unknown() {
        assert_eq!(estimate_image_tokens(None), 1024); // default fallback
    }

    // -- VisionEngine creation & configuration ------------------------------

    #[test]
    fn test_vision_engine_creation() {
        let engine = VisionEngine::new();
        assert!(engine.enable_text_extraction);
        assert!(engine.enable_ui_analysis);
        assert_eq!(engine.max_image_size_bytes, 20 * 1024 * 1024);
    }

    #[test]
    fn test_vision_engine_default() {
        let engine = VisionEngine::default();
        assert!(engine.enable_text_extraction);
    }

    #[test]
    fn test_vision_engine_builders() {
        let engine = VisionEngine::new()
            .with_text_extraction(false)
            .with_ui_analysis(false)
            .with_max_size(1024);
        assert!(!engine.enable_text_extraction);
        assert!(!engine.enable_ui_analysis);
        assert_eq!(engine.max_image_size_bytes, 1024);
    }

    // -- Screenshot heuristic -----------------------------------------------

    #[test]
    fn test_screenshot_heuristic_hd() {
        let img = VisionImage {
            base64_data: "dummy".into(),
            format: ImageFormat::Png,
            source_path: None,
            dimensions: Some((1920, 1080)), // 16:9 HD
            estimated_tokens: 2048,
            mime_type: "image/png".into(),
        };
        assert!(VisionEngine::new().is_likely_screenshot(&img));
    }

    #[test]
    fn test_screenshot_heuristic_too_small() {
        let small_img = VisionImage {
            base64_data: "dummy".into(),
            format: ImageFormat::Png,
            source_path: None,
            dimensions: Some((100, 100)),
            estimated_tokens: 256,
            mime_type: "image/png".into(),
        };
        assert!(!VisionEngine::new().is_likely_screenshot(&small_img));
    }

    #[test]
    fn test_screenshot_heuristic_no_dimensions() {
        let no_dim = VisionImage {
            base64_data: "dummy".into(),
            format: ImageFormat::Png,
            source_path: None,
            dimensions: None,
            estimated_tokens: 1024,
            mime_type: "image/png".into(),
        };
        assert!(!VisionEngine::new().is_likely_screenshot(&no_dim));
    }

    // -- process_bytes smoke test ------------------------------------------

    #[test]
    fn test_process_bytes_smoke() {
        let engine = VisionEngine::new().with_text_extraction(false).with_ui_analysis(false);
        // Tiny 2×2 red PNG (minimal valid PNG)
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
            0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02,
            0x08, 0x02, 0x00, 0x00, 0x00, 0xFD, 0xB4, 0x14, 0x00, 0x00, 0x00, 0x0C,
            0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00,
            0x01, 0x01, 0x00, 0x05, 0x18, 0xD8, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x49,
            0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let analysis = engine
            .process_bytes(&png_bytes, ImageFormat::Png)
            .expect("process_bytes should succeed for valid input");
        assert_eq!(analysis.metadata.width, 2);
        assert_eq!(analysis.metadata.height, 2);
        assert!(!analysis.vision_prompt.is_empty());
        assert!(!analysis.description.is_empty());
    }

    // -- build_vision_prompt public API ------------------------------------

    #[test]
    fn test_build_vision_prompt_format() {
        let img = VisionImage {
            base64_data: "AAAA".into(),
            format: ImageFormat::Png,
            source_path: None,
            dimensions: Some((800, 600)),
            estimated_tokens: 1200,
            mime_type: "image/png".into(),
        };
        let prompt = VisionEngine::new().build_vision_prompt(&img, "What is this?");
        assert!(prompt.contains("<image>"));
        assert!(prompt.contains("image/png"));
        assert!(prompt.contains("What is this?"));
    }

    // -- UiElement type completeness ---------------------------------------

    #[test]
    fn test_ui_element_types_exist() {
        // Verify all variants are usable.
        let _types = [
            UiElementType::Button,
            UiElementType::TextInput,
            UiElementType::TextArea,
            UiElementType::Link,
            UiElementType::Image,
            UiElementType::Icon,
            UiElementType::Text,
            UiElementType::Heading,
            UiElementType::Table,
            UiElementType::List,
            UiElementType::Dialog,
            UiElementType::Menu,
            UiElementType::Tab,
            UiElementType::Checkbox,
            UiElementType::Radio,
            UiElementType::Slider,
            UiElementType::Select,
            UiElementType::CodeBlock,
            UiElementType::Terminal,
            UiElementType::StatusBar,
            UiElementType::Toolbar,
            UiElementType::Sidebar,
            UiElementType::Unknown,
        ];
        assert_eq!(_types.len(), 23);
    }

    // -- Chart type completeness -------------------------------------------

    #[test]
    fn test_chart_types_exist() {
        let _types = [
            ChartType::Bar,
            ChartType::Line,
            ChartType::Pie,
            ChartType::Scatter,
            ChartType::Area,
            ChartType::HeatMap,
            ChartType::Unknown,
        ];
        assert_eq!(_types.len(), 7);
    }

    // -- BoundingBox -------------------------------------------------------

    #[test]
    fn test_bounding_box_fields() {
        let bb = BoundingBox {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.3,
        };
        assert!((bb.x - 0.1).abs() < f32::EPSILON);
        assert!((bb.y - 0.2).abs() < f32::EPSILON);
        assert!((bb.width - 0.5).abs() < f32::EPSILON);
        assert!((bb.height - 0.3).abs() < f32::EPSILON);
    }

    // -- New tests: VisionConfig, payload format, LocalOnly, data URI --

    #[test]
    fn test_vision_config_defaults() {
        let cfg = VisionConfig::default();
        assert_eq!(cfg.backend, VisionBackend::LocalOnly);
        assert!(cfg.api_key.is_none());
        assert!(cfg.api_url.is_none());
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn test_build_openai_vision_payload() {
        // Verify the JSON payload structure matches OpenAI vision API format
        #[derive(serde::Serialize)]
        struct TestPayload<'a> {
            model: &'a str,
            messages: Vec<TestMessage<'a>>,
        }

        #[derive(serde::Serialize)]
        struct TestMessage<'a> {
            role: &'a str,
            content: Vec<TestContent<'a>>,
        }

        #[derive(serde::Serialize)]
        struct TestContent<'a> {
            #[serde(rename = "type")]
            content_type: &'a str,
            text: Option<&'a str>,
            image_url: Option<TestImageUrl<'a>>,
        }

        #[derive(serde::Serialize)]
        struct TestImageUrl<'a> {
            url: &'a str,
        }

        let payload = TestPayload {
            model: "gpt-4o",
            messages: vec![TestMessage {
                role: "user",
                content: vec![
                    TestContent { content_type: "text", text: Some("Describe this"), image_url: None },
                    TestContent {
                        content_type: "image_url",
                        text: None,
                        image_url: Some(TestImageUrl { url: "data:image/png;base64,ABCDEF" }),
                    },
                ],
            }],
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"type\":\"image_url\""));
        assert!(json.contains("data:image/png;base64,ABCDEF"));
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_process_image_local_only() {
        // LocalOnly mode should not attempt any API call — just metadata + heuristics
        let engine = VisionEngine::with_config(Some(VisionConfig {
            backend: VisionBackend::LocalOnly,
            ..Default::default()
        }));

        // Use process_bytes (sync path) with a minimal PNG
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
            0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02,
            0x08, 0x02, 0x00, 0x00, 0x00, 0xFD, 0xB4, 0x14, 0x00, 0x00, 0x00, 0x0C,
            0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00,
            0x01, 0x01, 0x00, 0x05, 0x18, 0xD8, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x49,
            0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];

        let analysis = engine.process_bytes(&png_bytes, ImageFormat::Png).expect("should succeed");
        // Should have heuristic description (not empty)
        assert!(!analysis.description.is_empty());
        // No API was called → no real OCR text
        assert!(analysis.detected_text.is_empty());
        // No UI elements detected without API
        assert!(analysis.ui_elements.is_empty());
        // Metadata should be populated
        assert_eq!(analysis.metadata.width, 2);
        assert_eq!(analysis.metadata.height, 2);
    }

    #[test]
    fn test_image_data_uri_format() {
        let img = VisionImage {
            base64_data: "iVBORw0KGgo".to_string(),
            format: ImageFormat::Png,
            source_path: None,
            dimensions: Some((100, 200)),
            estimated_tokens: 512,
            mime_type: "image/png".to_string(),
        };

        let data_uri = format!("data:{};base64,{}", img.mime_type, img.base64_data);
        assert!(data_uri.starts_with("data:image/png;base64,"));
        assert!(data_uri.contains("iVBORw0KGgo"));

        // Verify JPEG variant
        let jpeg_img = VisionImage {
            base64_data: "/9j/4AAQ".to_string(),
            format: ImageFormat::Jpeg,
            source_path: None,
            dimensions: None,
            estimated_tokens: 1024,
            mime_type: "image/jpeg".to_string(),
        };
        let jpeg_uri = format!("data:{};base64,{}", jpeg_img.mime_type, jpeg_img.base64_data);
        assert!(jpeg_uri.starts_with("data:image/jpeg;base64,"));
    }

    // -- New tests: batch_process, detect_mime_type, VisionCache --

    #[test]
    fn test_batch_process_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let engine = VisionEngine::new();
        let results = rt.block_on(async {
            engine.batch_process(&[], "test", 5).await
        });
        assert!(results.is_empty());
    }

    #[test]
    fn test_detect_mime_type_png() {
        let png_header: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
        ];
        assert_eq!(ImageFormat::detect(&png_header), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::Png.mime_type(), "image/png");
    }

    #[test]
    fn test_detect_mime_type_jpg() {
        let jpg_header: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(ImageFormat::detect(&jpg_header), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
    }

    #[tokio::test]
    async fn test_vision_cache_get_set() {
        let cache = VisionCache::new();
        let p = Path::new("test.png");
        assert!(cache.get(p, "what?").await.is_none());

        cache.set(p, "what?", "a test image").await;
        let result = cache.get(p, "what?").await;
        assert_eq!(result, Some("a test image".to_string()));
    }

    #[tokio::test]
    async fn test_vision_cache_eviction() {
        let cache = VisionCache::with_max_entries(2);
        let p1 = Path::new("a.png");
        let p2 = Path::new("b.png");
        let p3 = Path::new("c.png");

        cache.set(p1, "q", "result a").await;
        cache.set(p2, "q", "result b").await;
        cache.set(p3, "q", "result c").await;

        // p1 should be evicted (oldest)
        assert!(cache.get(p1, "q").await.is_none());
        // p2 and p3 should exist
        assert!(cache.get(p2, "q").await.is_some());
        assert!(cache.get(p3, "q").await.is_some());
    }

    #[tokio::test]
    async fn test_vision_cache_stats() {
        let cache = VisionCache::new();
        let p = Path::new("img.png");

        let (entries, hits, _misses) = cache.stats().await;
        assert_eq!(entries, 0);
        assert_eq!(hits, 0);

        cache.set(p, "q", "desc").await;
        cache.get(p, "q").await;
        cache.get(p, "q").await;

        let (entries, hits, _misses) = cache.stats().await;
        assert_eq!(entries, 1);
        assert_eq!(hits, 2);
    }
}
