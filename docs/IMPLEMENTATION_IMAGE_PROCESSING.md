# IMPLEMENTATION - CPU-Based Image Processing (OCR + Vision)

## Status: IN PROGRESS

**Status**: Phase 1 - Foundation (Sub-phase 1.1 Complete)
**Version**: v8.6.0-image-processing (planned)
**Date Started**: TBD
**Quality Score**: 0/10

**Test Coverage**:
- [ ] OCR model tests
- [ ] Florence model tests
- [ ] Image preprocessing tests
- [ ] API endpoint tests
- [ ] Integration tests

---

## Overview

Implementation plan for CPU-based image processing in fabstir-llm-node. This enables hosts to process images for RAG without requiring GPU VRAM (reserved for LLM inference). Supports decentralized P2P AI network requirements - no external API calls.

**Two new endpoints:**
- `POST /v1/ocr` - Extract text from images using PaddleOCR (CPU)
- `POST /v1/describe-image` - Generate image descriptions using Florence-2 (CPU)

**Timeline**: ~18 hours total
**Location**: `fabstir-llm-node/` (Rust project)
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Version**: v8.6.0+

**Key Constraints:**
- **CPU-only execution** - No GPU usage (VRAM reserved for LLM)
- **32GB RAM minimum** - Assumes hosts have adequate memory
- **Decentralized** - No external API calls
- **Follow existing patterns** - Mirror `/v1/embed` implementation

**References:**
- Embedding Endpoint Pattern: `src/api/embed/`
- ONNX Model Loading: `src/embeddings/onnx_model.rs`
- Model Manager Pattern: `src/embeddings/model_manager.rs`

---

## Dependencies Required

### Cargo.toml Updates
```toml
[dependencies]
# Existing dependencies...

# Image Processing (NEW)
image = "0.25"                                    # Image loading/preprocessing/resizing

# Multipart Form Handling (NEW)
axum-extra = { version = "0.9", features = ["multipart"] }  # File uploads

# Already present (no changes needed):
# ort = { version = "2.0.0-rc.10", features = ["download-binaries", "cuda"] }
# ndarray = "0.16"
# base64 = "0.22"
# tokenizers = "0.20"
```

**Note**: Uses existing `ort` crate for ONNX runtime. CPU-only execution via `CPUExecutionProvider`.

---

## Model Requirements

### PaddleOCR Models (~15MB total)
```
models/paddleocr-onnx/
├── det_model.onnx           # Text detection (~3MB)
├── rec_model.onnx           # Text recognition (~10MB)
├── cls_model.onnx           # Text classifier (~2MB, optional)
├── ppocr_keys_v1.txt        # Character dictionary
└── VERSION                  # Version tracking
```

### Florence-2 Models (~2GB total)
```
models/florence-2-onnx/
├── encoder.onnx             # Vision encoder
├── decoder.onnx             # Language decoder
├── tokenizer.json           # Tokenizer config
└── VERSION                  # Version tracking
```

---

## Phase 1: Foundation (2 hours)

### Sub-phase 1.1: Add Dependencies ✅

**Goal**: Add required crates to Cargo.toml

**Status**: COMPLETE (2025-12-30)

#### Tasks
- [x] Add `image = "0.25"` to Cargo.toml
- [x] Add `axum-extra = { version = "0.9", features = ["multipart"] }` to Cargo.toml
- [x] Verify existing `ort`, `ndarray`, `base64`, `tokenizers` dependencies
- [x] Run `cargo check` to verify dependencies compile
- [x] Run existing tests to ensure no regressions (310 passed, 2 env-dependent failures pre-existing)

**Test Files:**
- Run `cargo test --lib` - Ensure no regressions

**Implementation Files:**
- `Cargo.toml` - Add dependencies
  - Added `image = "0.25"` (line 52)
  - Added `axum-extra = { version = "0.9", features = ["multipart"] }` (line 35)

---

### Sub-phase 1.2: Create Module Structure

**Goal**: Create stub files for all new modules

#### Tasks
- [ ] Create `src/vision/mod.rs` with submodule declarations
- [ ] Create `src/vision/model_manager.rs` stub
- [ ] Create `src/vision/ocr/mod.rs` stub
- [ ] Create `src/vision/ocr/model.rs` stub
- [ ] Create `src/vision/ocr/preprocessing.rs` stub
- [ ] Create `src/vision/florence/mod.rs` stub
- [ ] Create `src/vision/florence/model.rs` stub
- [ ] Create `src/vision/florence/preprocessing.rs` stub
- [ ] Create `src/api/ocr/mod.rs` stub
- [ ] Create `src/api/ocr/handler.rs` stub
- [ ] Create `src/api/ocr/request.rs` stub
- [ ] Create `src/api/ocr/response.rs` stub
- [ ] Create `src/api/describe_image/mod.rs` stub
- [ ] Create `src/api/describe_image/handler.rs` stub
- [ ] Create `src/api/describe_image/request.rs` stub
- [ ] Create `src/api/describe_image/response.rs` stub
- [ ] Add `pub mod vision;` to `src/lib.rs`
- [ ] Add `pub mod ocr;` and `pub mod describe_image;` to `src/api/mod.rs`
- [ ] Run `cargo check` to verify module structure

**Test Files:**
- `tests/vision/test_module_structure.rs` - Verify modules export correctly
  - Test vision module imports
  - Test API module imports

**Implementation Files:**
- `src/vision/mod.rs`
  ```rust
  pub mod model_manager;
  pub mod ocr;
  pub mod florence;

  pub use model_manager::VisionModelManager;
  ```

- `src/api/ocr/mod.rs`
  ```rust
  pub mod handler;
  pub mod request;
  pub mod response;

  pub use handler::ocr_handler;
  pub use request::OcrRequest;
  pub use response::OcrResponse;
  ```

---

### Sub-phase 1.3: Define Request/Response Types

**Goal**: Define API types with validation

#### Tasks
- [ ] Write tests for OcrRequest serialization/deserialization
- [ ] Write tests for OcrRequest validation (image required, format valid, chain_id valid)
- [ ] Write tests for OcrResponse serialization
- [ ] Write tests for DescribeImageRequest serialization/deserialization
- [ ] Write tests for DescribeImageRequest validation
- [ ] Write tests for DescribeImageResponse serialization
- [ ] Implement OcrRequest struct with validation
- [ ] Implement OcrResponse struct
- [ ] Implement DescribeImageRequest struct with validation
- [ ] Implement DescribeImageResponse struct
- [ ] Add TextRegion and BoundingBox types
- [ ] Add DetectedObject and ImageAnalysis types

**Test Files:**
- `tests/api/test_ocr_request.rs` (max 300 lines)
  - Test OcrRequest deserialization with all fields
  - Test OcrRequest with defaults
  - Test validation: missing image
  - Test validation: invalid format
  - Test validation: invalid chain_id
  - Test validation: image too large (>10MB base64)

- `tests/api/test_ocr_response.rs` (max 200 lines)
  - Test OcrResponse serialization
  - Test TextRegion serialization
  - Test BoundingBox serialization

- `tests/api/test_describe_image_request.rs` (max 300 lines)
  - Test DescribeImageRequest deserialization
  - Test validation: detail levels (brief, detailed, comprehensive)
  - Test validation: maxTokens range (10-500)

- `tests/api/test_describe_image_response.rs` (max 200 lines)
  - Test DescribeImageResponse serialization
  - Test DetectedObject serialization
  - Test ImageAnalysis serialization

**Implementation Files:**
- `src/api/ocr/request.rs` (max 200 lines)
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct OcrRequest {
      /// Base64-encoded image data
      pub image: Option<String>,

      /// Image format hint (png, jpg, webp)
      #[serde(default = "default_format")]
      pub format: String,

      /// Language hint for OCR (en, zh, ja, ko)
      #[serde(default = "default_language")]
      pub language: String,

      /// Chain ID for pricing/metering
      #[serde(default = "default_chain_id")]
      pub chain_id: u64,
  }

  impl OcrRequest {
      pub fn validate(&self) -> Result<(), ApiError> {
          // Validate image is provided
          // Validate format is supported (png, jpg, jpeg, webp, gif)
          // Validate language is supported (en, zh, ja, ko)
          // Validate image size (max 10MB when decoded)
          // Validate chain_id (84532 or 5611)
      }
  }
  ```

- `src/api/ocr/response.rs` (max 150 lines)
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct OcrResponse {
      pub text: String,
      pub confidence: f32,
      pub regions: Vec<TextRegion>,
      pub processing_time_ms: u64,
      pub model: String,
      pub provider: String,
      pub chain_id: u64,
      pub chain_name: String,
      pub native_token: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct TextRegion {
      pub text: String,
      pub confidence: f32,
      pub bounding_box: BoundingBox,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct BoundingBox {
      pub x: u32,
      pub y: u32,
      pub width: u32,
      pub height: u32,
  }
  ```

- `src/api/describe_image/request.rs` (max 200 lines)
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DescribeImageRequest {
      pub image: Option<String>,

      #[serde(default = "default_format")]
      pub format: String,

      /// Detail level: brief, detailed, comprehensive
      #[serde(default = "default_detail")]
      pub detail: String,

      /// Custom prompt for description
      #[serde(default)]
      pub prompt: Option<String>,

      /// Maximum tokens in response (10-500)
      #[serde(default = "default_max_tokens")]
      pub max_tokens: usize,

      #[serde(default = "default_chain_id")]
      pub chain_id: u64,
  }
  ```

- `src/api/describe_image/response.rs` (max 150 lines)
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DescribeImageResponse {
      pub description: String,
      pub objects: Vec<DetectedObject>,
      pub analysis: ImageAnalysis,
      pub processing_time_ms: u64,
      pub model: String,
      pub provider: String,
      pub chain_id: u64,
      pub chain_name: String,
      pub native_token: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DetectedObject {
      pub label: String,
      pub confidence: f32,
      pub bounding_box: Option<BoundingBox>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct ImageAnalysis {
      pub width: u32,
      pub height: u32,
      pub dominant_colors: Vec<String>,
      pub scene_type: Option<String>,
  }
  ```

---

### Sub-phase 1.4: Create Model Download Scripts

**Goal**: Create scripts to download ONNX models

#### Tasks
- [ ] Create `scripts/download_ocr_models.sh`
- [ ] Create `scripts/download_florence_model.sh`
- [ ] Test PaddleOCR model download
- [ ] Test Florence-2 model download
- [ ] Document model sources and versions

**Implementation Files:**
- `scripts/download_ocr_models.sh`
  ```bash
  #!/bin/bash
  # Download PaddleOCR ONNX models for CPU-based OCR

  set -e

  MODEL_DIR="./models/paddleocr-onnx"
  mkdir -p "${MODEL_DIR}"

  echo "Downloading PaddleOCR ONNX models..."

  # Detection model (PP-OCRv4)
  curl -L -o "${MODEL_DIR}/det_model.onnx" \
    "https://huggingface.co/tomaarsen/paddleocr-onnx/resolve/main/det_model.onnx"

  # Recognition model (PP-OCRv4)
  curl -L -o "${MODEL_DIR}/rec_model.onnx" \
    "https://huggingface.co/tomaarsen/paddleocr-onnx/resolve/main/rec_model.onnx"

  # Character dictionary
  curl -L -o "${MODEL_DIR}/ppocr_keys_v1.txt" \
    "https://huggingface.co/tomaarsen/paddleocr-onnx/resolve/main/ppocr_keys_v1.txt"

  # Version file
  echo "PP-OCRv4-ONNX" > "${MODEL_DIR}/VERSION"

  echo "PaddleOCR models downloaded to ${MODEL_DIR}"
  ls -la "${MODEL_DIR}"
  ```

- `scripts/download_florence_model.sh`
  ```bash
  #!/bin/bash
  # Download Florence-2-base ONNX model for CPU-based image description

  set -e

  MODEL_DIR="./models/florence-2-onnx"
  mkdir -p "${MODEL_DIR}"

  echo "Downloading Florence-2 ONNX models..."

  # Florence-2-base ONNX from HuggingFace
  # Note: Check onnx-community/Florence-2-base-ft for latest
  curl -L -o "${MODEL_DIR}/encoder.onnx" \
    "https://huggingface.co/onnx-community/Florence-2-base-ft/resolve/main/onnx/vision_encoder.onnx"

  curl -L -o "${MODEL_DIR}/decoder.onnx" \
    "https://huggingface.co/onnx-community/Florence-2-base-ft/resolve/main/onnx/decoder_model.onnx"

  curl -L -o "${MODEL_DIR}/tokenizer.json" \
    "https://huggingface.co/onnx-community/Florence-2-base-ft/resolve/main/tokenizer.json"

  # Version file
  echo "Florence-2-base-ONNX" > "${MODEL_DIR}/VERSION"

  echo "Florence-2 models downloaded to ${MODEL_DIR}"
  ls -la "${MODEL_DIR}"
  ```

---

## Phase 2: Image Preprocessing (2 hours)

### Sub-phase 2.1: Image Loading

**Goal**: Load images from base64 and multipart form data

#### Tasks
- [ ] Write tests for base64 image decoding (PNG, JPG, WebP)
- [ ] Write tests for invalid base64 rejection
- [ ] Write tests for unsupported format rejection
- [ ] Write tests for image size validation (max 10MB)
- [ ] Write tests for multipart image extraction
- [ ] Implement `decode_base64_image` function
- [ ] Implement `extract_multipart_image` function
- [ ] Add format detection from magic bytes
- [ ] Add image dimension extraction

**Test Files:**
- `tests/vision/test_image_loading.rs` (max 300 lines)
  - Test decode_base64_image with PNG
  - Test decode_base64_image with JPG
  - Test decode_base64_image with WebP
  - Test invalid base64 rejection
  - Test corrupted image rejection
  - Test oversized image rejection (>10MB)
  - Test format detection from magic bytes

**Implementation Files:**
- `src/vision/image_utils.rs` (max 200 lines)
  ```rust
  use image::{DynamicImage, ImageFormat};
  use base64::{Engine as _, engine::general_purpose::STANDARD};

  pub fn decode_base64_image(base64_str: &str) -> Result<DynamicImage, ImageError> {
      let bytes = STANDARD.decode(base64_str)?;

      // Validate size (max 10MB)
      if bytes.len() > 10 * 1024 * 1024 {
          return Err(ImageError::TooLarge);
      }

      // Detect format from magic bytes
      let format = detect_format(&bytes)?;

      // Load image
      let img = image::load_from_memory_with_format(&bytes, format)?;

      Ok(img)
  }

  fn detect_format(bytes: &[u8]) -> Result<ImageFormat, ImageError> {
      match bytes {
          [0x89, 0x50, 0x4E, 0x47, ..] => Ok(ImageFormat::Png),
          [0xFF, 0xD8, 0xFF, ..] => Ok(ImageFormat::Jpeg),
          [0x52, 0x49, 0x46, 0x46, ..] => Ok(ImageFormat::WebP),
          [0x47, 0x49, 0x46, ..] => Ok(ImageFormat::Gif),
          _ => Err(ImageError::UnsupportedFormat),
      }
  }
  ```

---

### Sub-phase 2.2: OCR Preprocessing

**Goal**: Preprocess images for PaddleOCR input

#### Tasks
- [ ] Write tests for image resizing to 640x640
- [ ] Write tests for normalization (mean subtraction, std division)
- [ ] Write tests for RGB to BGR conversion (if needed)
- [ ] Write tests for ndarray conversion
- [ ] Implement `preprocess_for_ocr` function
- [ ] Add aspect ratio preservation with padding
- [ ] Add batch dimension expansion

**Test Files:**
- `tests/vision/test_ocr_preprocessing.rs` (max 250 lines)
  - Test resize to 640x640
  - Test aspect ratio preservation
  - Test normalization values
  - Test ndarray shape [1, 3, 640, 640]
  - Test various input sizes

**Implementation Files:**
- `src/vision/ocr/preprocessing.rs` (max 200 lines)
  ```rust
  use image::DynamicImage;
  use ndarray::{Array4, s};

  /// Target size for PaddleOCR detection model
  const OCR_INPUT_SIZE: u32 = 640;

  /// Mean values for normalization (ImageNet)
  const MEAN: [f32; 3] = [0.485, 0.456, 0.406];

  /// Std values for normalization (ImageNet)
  const STD: [f32; 3] = [0.229, 0.224, 0.225];

  pub fn preprocess_for_ocr(img: &DynamicImage) -> Array4<f32> {
      // 1. Resize with aspect ratio preservation
      let resized = resize_with_padding(img, OCR_INPUT_SIZE);

      // 2. Convert to RGB
      let rgb = resized.to_rgb8();

      // 3. Normalize: (pixel / 255.0 - mean) / std
      // 4. Convert to ndarray [1, 3, H, W] (NCHW format)
      // 5. Return preprocessed tensor
  }
  ```

---

### Sub-phase 2.3: Florence Preprocessing

**Goal**: Preprocess images for Florence-2 input

#### Tasks
- [ ] Write tests for image resizing to 768x768
- [ ] Write tests for normalization (CLIP-style)
- [ ] Write tests for ndarray conversion
- [ ] Implement `preprocess_for_florence` function
- [ ] Add center crop option
- [ ] Add batch dimension expansion

**Test Files:**
- `tests/vision/test_florence_preprocessing.rs` (max 250 lines)
  - Test resize to 768x768
  - Test center crop
  - Test normalization values (CLIP-style)
  - Test ndarray shape [1, 3, 768, 768]

**Implementation Files:**
- `src/vision/florence/preprocessing.rs` (max 200 lines)
  ```rust
  use image::DynamicImage;
  use ndarray::Array4;

  /// Target size for Florence-2 vision encoder
  const FLORENCE_INPUT_SIZE: u32 = 768;

  /// CLIP normalization values
  const MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
  const STD: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

  pub fn preprocess_for_florence(img: &DynamicImage) -> Array4<f32> {
      // 1. Resize to 768x768 (center crop or letterbox)
      // 2. Convert to RGB
      // 3. Normalize with CLIP values
      // 4. Convert to ndarray [1, 3, 768, 768]
  }
  ```

---

## Phase 3: PaddleOCR Integration (4 hours)

### Sub-phase 3.1: Load OCR Detection Model

**Goal**: Load PaddleOCR detection model with ONNX Runtime (CPU-only)

#### Tasks
- [ ] Write tests for detection model loading
- [ ] Write tests for CPU-only execution provider
- [ ] Write tests for model input/output shapes
- [ ] Write tests for missing model file error
- [ ] Implement `OcrDetectionModel` struct
- [ ] Load model with `CPUExecutionProvider::default().build()`
- [ ] Configure thread count and optimization level
- [ ] Validate model input/output shapes

**Test Files:**
- `tests/vision/test_ocr_detection_model.rs` (max 300 lines)
  - Test model loads successfully
  - Test CPU execution provider is used
  - Test input shape [1, 3, 640, 640]
  - Test output shape validation
  - Test missing model file error
  - Test inference on test image

**Implementation Files:**
- `src/vision/ocr/detection.rs` (max 300 lines)
  ```rust
  use ort::{Session, CPUExecutionProvider, GraphOptimizationLevel};
  use std::sync::{Arc, Mutex};

  pub struct OcrDetectionModel {
      session: Arc<Mutex<Session>>,
      input_name: String,
      output_name: String,
  }

  impl OcrDetectionModel {
      pub async fn new(model_path: &str) -> Result<Self> {
          // Force CPU-only execution
          let session = Session::builder()
              .with_execution_providers([CPUExecutionProvider::default().build()])
              .with_optimization_level(GraphOptimizationLevel::Level3)
              .with_intra_threads(4)
              .commit_from_file(model_path)?;

          // Get input/output names
          // Validate shapes

          Ok(Self { session, input_name, output_name })
      }

      pub fn detect(&self, input: &Array4<f32>) -> Result<Vec<TextBox>> {
          // Run inference
          // Parse detection output
          // Return text bounding boxes
      }
  }

  pub struct TextBox {
      pub x: f32,
      pub y: f32,
      pub width: f32,
      pub height: f32,
      pub confidence: f32,
  }
  ```

---

### Sub-phase 3.2: Load OCR Recognition Model

**Goal**: Load PaddleOCR recognition model with ONNX Runtime (CPU-only)

#### Tasks
- [ ] Write tests for recognition model loading
- [ ] Write tests for character dictionary loading
- [ ] Write tests for model input shape (variable width)
- [ ] Write tests for character decoding
- [ ] Implement `OcrRecognitionModel` struct
- [ ] Load recognition model (CPU-only)
- [ ] Load character dictionary from ppocr_keys_v1.txt
- [ ] Implement CTC decoding for output

**Test Files:**
- `tests/vision/test_ocr_recognition_model.rs` (max 300 lines)
  - Test model loads successfully
  - Test dictionary loads (6623+ characters)
  - Test input shape [1, 3, 48, W]
  - Test character decoding
  - Test sample text recognition

**Implementation Files:**
- `src/vision/ocr/recognition.rs` (max 350 lines)
  ```rust
  pub struct OcrRecognitionModel {
      session: Arc<Mutex<Session>>,
      dictionary: Vec<char>,
      input_name: String,
      output_name: String,
  }

  impl OcrRecognitionModel {
      pub async fn new(model_path: &str, dict_path: &str) -> Result<Self> {
          // Load model (CPU-only)
          // Load character dictionary
      }

      pub fn recognize(&self, text_box_image: &Array4<f32>) -> Result<RecognizedText> {
          // Run inference
          // CTC decode output
          // Return recognized text with confidence
      }
  }

  pub struct RecognizedText {
      pub text: String,
      pub confidence: f32,
  }
  ```

---

### Sub-phase 3.3: Full OCR Pipeline

**Goal**: Combine detection and recognition into complete OCR pipeline

#### Tasks
- [ ] Write tests for end-to-end OCR pipeline
- [ ] Write tests for multiple text regions
- [ ] Write tests for empty image (no text)
- [ ] Write tests for confidence thresholding
- [ ] Implement `PaddleOcrModel` struct combining detection + recognition
- [ ] Implement text box cropping
- [ ] Implement result aggregation
- [ ] Add confidence filtering

**Test Files:**
- `tests/vision/test_ocr_pipeline.rs` (max 400 lines)
  - Test end-to-end OCR on sample document
  - Test multiple text regions detected
  - Test empty image returns empty text
  - Test confidence threshold filtering
  - Test processing time within target (<3s)

**Implementation Files:**
- `src/vision/ocr/model.rs` (max 400 lines)
  ```rust
  pub struct PaddleOcrModel {
      detector: OcrDetectionModel,
      recognizer: OcrRecognitionModel,
      confidence_threshold: f32,
  }

  impl PaddleOcrModel {
      pub async fn new(model_dir: &str) -> Result<Self> {
          let detector = OcrDetectionModel::new(&format!("{}/det_model.onnx", model_dir)).await?;
          let recognizer = OcrRecognitionModel::new(
              &format!("{}/rec_model.onnx", model_dir),
              &format!("{}/ppocr_keys_v1.txt", model_dir),
          ).await?;

          Ok(Self {
              detector,
              recognizer,
              confidence_threshold: 0.5,
          })
      }

      pub fn process(&self, image: &DynamicImage) -> Result<OcrResult> {
          let start = std::time::Instant::now();

          // 1. Preprocess image for detection
          let det_input = preprocess_for_detection(image);

          // 2. Detect text boxes
          let text_boxes = self.detector.detect(&det_input)?;

          // 3. For each text box, crop and recognize
          let mut regions = Vec::new();
          for text_box in text_boxes {
              if text_box.confidence < self.confidence_threshold {
                  continue;
              }

              let cropped = crop_text_box(image, &text_box);
              let rec_input = preprocess_for_recognition(&cropped);
              let recognized = self.recognizer.recognize(&rec_input)?;

              regions.push(TextRegion {
                  text: recognized.text,
                  confidence: recognized.confidence,
                  bounding_box: text_box.into(),
              });
          }

          // 4. Aggregate results
          let full_text = regions.iter().map(|r| &r.text).collect::<Vec<_>>().join(" ");
          let avg_confidence = regions.iter().map(|r| r.confidence).sum::<f32>() / regions.len().max(1) as f32;

          Ok(OcrResult {
              text: full_text,
              confidence: avg_confidence,
              regions,
              processing_time_ms: start.elapsed().as_millis() as u64,
          })
      }
  }
  ```

---

## Phase 4: Florence-2 Integration (4 hours)

### Sub-phase 4.1: Load Florence Vision Encoder

**Goal**: Load Florence-2 vision encoder with ONNX Runtime (CPU-only)

#### Tasks
- [ ] Write tests for encoder model loading
- [ ] Write tests for CPU-only execution
- [ ] Write tests for input shape [1, 3, 768, 768]
- [ ] Write tests for output embedding shape
- [ ] Implement `FlorenceEncoder` struct
- [ ] Load encoder model (CPU-only)
- [ ] Validate input/output shapes

**Test Files:**
- `tests/vision/test_florence_encoder.rs` (max 250 lines)
  - Test encoder loads successfully
  - Test CPU execution provider
  - Test input shape validation
  - Test output embedding extraction
  - Test inference on test image

**Implementation Files:**
- `src/vision/florence/encoder.rs` (max 250 lines)
  ```rust
  pub struct FlorenceEncoder {
      session: Arc<Mutex<Session>>,
      input_name: String,
      output_name: String,
  }

  impl FlorenceEncoder {
      pub async fn new(model_path: &str) -> Result<Self> {
          let session = Session::builder()
              .with_execution_providers([CPUExecutionProvider::default().build()])
              .with_optimization_level(GraphOptimizationLevel::Level3)
              .with_intra_threads(4)
              .commit_from_file(model_path)?;

          Ok(Self { session, input_name, output_name })
      }

      pub fn encode(&self, image: &Array4<f32>) -> Result<Array2<f32>> {
          // Run vision encoder
          // Return image embeddings
      }
  }
  ```

---

### Sub-phase 4.2: Load Florence Decoder

**Goal**: Load Florence-2 language decoder with ONNX Runtime (CPU-only)

#### Tasks
- [ ] Write tests for decoder model loading
- [ ] Write tests for tokenizer loading
- [ ] Write tests for text generation
- [ ] Write tests for max token limit
- [ ] Implement `FlorenceDecoder` struct
- [ ] Load decoder model (CPU-only)
- [ ] Load tokenizer from tokenizer.json
- [ ] Implement autoregressive generation

**Test Files:**
- `tests/vision/test_florence_decoder.rs` (max 300 lines)
  - Test decoder loads successfully
  - Test tokenizer loads (vocab size)
  - Test text generation from embeddings
  - Test max token limit respected
  - Test special token handling

**Implementation Files:**
- `src/vision/florence/decoder.rs` (max 350 lines)
  ```rust
  use tokenizers::Tokenizer;

  pub struct FlorenceDecoder {
      session: Arc<Mutex<Session>>,
      tokenizer: Arc<Tokenizer>,
      max_tokens: usize,
  }

  impl FlorenceDecoder {
      pub async fn new(model_path: &str, tokenizer_path: &str) -> Result<Self> {
          let session = Session::builder()
              .with_execution_providers([CPUExecutionProvider::default().build()])
              .with_optimization_level(GraphOptimizationLevel::Level3)
              .with_intra_threads(4)
              .commit_from_file(model_path)?;

          let tokenizer = Tokenizer::from_file(tokenizer_path)?;

          Ok(Self { session, tokenizer, max_tokens: 150 })
      }

      pub fn generate(&self, image_embeddings: &Array2<f32>, prompt: Option<&str>) -> Result<String> {
          // Tokenize prompt if provided
          // Autoregressive generation loop
          // Decode tokens to text
      }
  }
  ```

---

### Sub-phase 4.3: Full Florence Pipeline

**Goal**: Combine encoder and decoder into complete vision pipeline

#### Tasks
- [ ] Write tests for end-to-end image description
- [ ] Write tests for different detail levels
- [ ] Write tests for custom prompts
- [ ] Write tests for object detection output
- [ ] Implement `FlorenceModel` struct
- [ ] Implement image captioning
- [ ] Implement object detection parsing
- [ ] Add detail level handling

**Test Files:**
- `tests/vision/test_florence_pipeline.rs` (max 400 lines)
  - Test end-to-end image description
  - Test brief vs detailed vs comprehensive
  - Test custom prompt generation
  - Test object detection output
  - Test processing time within target (<5s)

**Implementation Files:**
- `src/vision/florence/model.rs` (max 400 lines)
  ```rust
  pub struct FlorenceModel {
      encoder: FlorenceEncoder,
      decoder: FlorenceDecoder,
  }

  impl FlorenceModel {
      pub async fn new(model_dir: &str) -> Result<Self> {
          let encoder = FlorenceEncoder::new(&format!("{}/encoder.onnx", model_dir)).await?;
          let decoder = FlorenceDecoder::new(
              &format!("{}/decoder.onnx", model_dir),
              &format!("{}/tokenizer.json", model_dir),
          ).await?;

          Ok(Self { encoder, decoder })
      }

      pub fn describe(&self, image: &DynamicImage, detail: &str, prompt: Option<&str>) -> Result<DescriptionResult> {
          let start = std::time::Instant::now();

          // 1. Preprocess image
          let input = preprocess_for_florence(image);

          // 2. Encode image
          let embeddings = self.encoder.encode(&input)?;

          // 3. Generate description
          let task_prompt = match detail {
              "brief" => "<CAPTION>",
              "detailed" => "<DETAILED_CAPTION>",
              "comprehensive" => "<MORE_DETAILED_CAPTION>",
              _ => "<CAPTION>",
          };

          let full_prompt = prompt.map(|p| format!("{} {}", task_prompt, p))
              .unwrap_or_else(|| task_prompt.to_string());

          let description = self.decoder.generate(&embeddings, Some(&full_prompt))?;

          // 4. Parse objects if present
          let objects = parse_objects(&description);

          Ok(DescriptionResult {
              description,
              objects,
              processing_time_ms: start.elapsed().as_millis() as u64,
          })
      }
  }
  ```

---

## Phase 5: API Handlers (3 hours)

### Sub-phase 5.1: Vision Model Manager

**Goal**: Create model manager to hold OCR and Florence models

#### Tasks
- [ ] Write tests for VisionModelManager initialization
- [ ] Write tests for model availability checks
- [ ] Write tests for graceful handling of missing models
- [ ] Implement `VisionModelManager` struct
- [ ] Add optional model loading
- [ ] Add model availability methods
- [ ] Add list_models method

**Test Files:**
- `tests/vision/test_model_manager.rs` (max 250 lines)
  - Test manager initializes with both models
  - Test manager initializes with OCR only
  - Test manager initializes with Florence only
  - Test get_ocr_model returns correct model
  - Test get_florence_model returns correct model
  - Test list_models returns available models

**Implementation Files:**
- `src/vision/model_manager.rs` (max 300 lines)
  ```rust
  pub struct VisionModelManager {
      ocr_model: Option<Arc<PaddleOcrModel>>,
      florence_model: Option<Arc<FlorenceModel>>,
  }

  impl VisionModelManager {
      pub async fn new(
          ocr_model_dir: Option<&str>,
          florence_model_dir: Option<&str>,
      ) -> Result<Self> {
          let ocr_model = if let Some(dir) = ocr_model_dir {
              match PaddleOcrModel::new(dir).await {
                  Ok(model) => Some(Arc::new(model)),
                  Err(e) => {
                      tracing::warn!("Failed to load OCR model: {}", e);
                      None
                  }
              }
          } else {
              None
          };

          let florence_model = if let Some(dir) = florence_model_dir {
              match FlorenceModel::new(dir).await {
                  Ok(model) => Some(Arc::new(model)),
                  Err(e) => {
                      tracing::warn!("Failed to load Florence model: {}", e);
                      None
                  }
              }
          } else {
              None
          };

          Ok(Self { ocr_model, florence_model })
      }

      pub fn get_ocr_model(&self) -> Option<Arc<PaddleOcrModel>> {
          self.ocr_model.clone()
      }

      pub fn get_florence_model(&self) -> Option<Arc<FlorenceModel>> {
          self.florence_model.clone()
      }

      pub fn list_models(&self) -> Vec<VisionModelInfo> {
          let mut models = Vec::new();
          if self.ocr_model.is_some() {
              models.push(VisionModelInfo {
                  name: "paddleocr".to_string(),
                  model_type: "ocr".to_string(),
                  available: true,
              });
          }
          if self.florence_model.is_some() {
              models.push(VisionModelInfo {
                  name: "florence-2".to_string(),
                  model_type: "vision".to_string(),
                  available: true,
              });
          }
          models
      }
  }
  ```

---

### Sub-phase 5.2: OCR Handler

**Goal**: Implement POST /v1/ocr HTTP handler

#### Tasks
- [ ] Write tests for OCR handler with JSON request
- [ ] Write tests for OCR handler with multipart request
- [ ] Write tests for validation errors
- [ ] Write tests for model not available error (503)
- [ ] Write tests for chain context in response
- [ ] Implement `ocr_handler` function
- [ ] Add multipart support
- [ ] Add proper error responses

**Test Files:**
- `tests/api/test_ocr_endpoint.rs` (max 400 lines)
  - Test successful OCR with JSON request
  - Test successful OCR with multipart request
  - Test validation error (missing image)
  - Test validation error (invalid format)
  - Test model not available (503)
  - Test chain context in response
  - Test processing time tracking

**Implementation Files:**
- `src/api/ocr/handler.rs` (max 300 lines)
  ```rust
  use axum::{
      extract::{State, Json},
      http::StatusCode,
  };
  use axum_extra::extract::Multipart;

  /// POST /v1/ocr - JSON handler
  pub async fn ocr_handler(
      State(state): State<AppState>,
      Json(request): Json<OcrRequest>,
  ) -> Result<Json<OcrResponse>, (StatusCode, String)> {
      // 1. Validate request
      if let Err(e) = request.validate() {
          return Err((StatusCode::BAD_REQUEST, e.to_string()));
      }

      // 2. Get OCR model from state
      let manager = state.vision_model_manager.read().await;
      let manager = manager.as_ref().ok_or_else(|| {
          (StatusCode::SERVICE_UNAVAILABLE, "Vision service not available".to_string())
      })?;

      let ocr_model = manager.get_ocr_model().ok_or_else(|| {
          (StatusCode::SERVICE_UNAVAILABLE, "OCR model not loaded".to_string())
      })?;

      // 3. Decode image
      let image = decode_base64_image(&request.image.unwrap_or_default())
          .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

      // 4. Run OCR
      let result = ocr_model.process(&image)
          .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

      // 5. Build response with chain context
      let (chain_name, native_token) = match request.chain_id {
          84532 => ("Base Sepolia", "ETH"),
          5611 => ("opBNB Testnet", "BNB"),
          _ => ("Base Sepolia", "ETH"),
      };

      Ok(Json(OcrResponse {
          text: result.text,
          confidence: result.confidence,
          regions: result.regions,
          processing_time_ms: result.processing_time_ms,
          model: "paddleocr".to_string(),
          provider: "host".to_string(),
          chain_id: request.chain_id,
          chain_name: chain_name.to_string(),
          native_token: native_token.to_string(),
      }))
  }

  /// POST /v1/ocr - Multipart handler
  pub async fn ocr_multipart_handler(
      State(state): State<AppState>,
      mut multipart: Multipart,
  ) -> Result<Json<OcrResponse>, (StatusCode, String)> {
      // Extract image from multipart
      // Convert to OcrRequest
      // Call ocr_handler logic
  }
  ```

---

### Sub-phase 5.3: Describe Image Handler

**Goal**: Implement POST /v1/describe-image HTTP handler

#### Tasks
- [ ] Write tests for describe-image handler with JSON request
- [ ] Write tests for describe-image handler with multipart request
- [ ] Write tests for detail levels (brief, detailed, comprehensive)
- [ ] Write tests for custom prompt handling
- [ ] Write tests for model not available error (503)
- [ ] Implement `describe_image_handler` function
- [ ] Add multipart support
- [ ] Add proper error responses

**Test Files:**
- `tests/api/test_describe_image_endpoint.rs` (max 400 lines)
  - Test successful description with JSON request
  - Test successful description with multipart request
  - Test brief detail level
  - Test detailed detail level
  - Test comprehensive detail level
  - Test custom prompt
  - Test model not available (503)
  - Test chain context in response

**Implementation Files:**
- `src/api/describe_image/handler.rs` (max 300 lines)
  ```rust
  /// POST /v1/describe-image - JSON handler
  pub async fn describe_image_handler(
      State(state): State<AppState>,
      Json(request): Json<DescribeImageRequest>,
  ) -> Result<Json<DescribeImageResponse>, (StatusCode, String)> {
      // 1. Validate request
      // 2. Get Florence model from state
      // 3. Decode image
      // 4. Run description
      // 5. Build response with chain context
  }
  ```

---

## Phase 6: Integration (2 hours)

### Sub-phase 6.1: Update AppState

**Goal**: Add VisionModelManager to AppState

#### Tasks
- [ ] Add `vision_model_manager` field to AppState
- [ ] Update AppState::new_for_test() to include vision manager
- [ ] Add setter method for vision model manager

**Implementation Files:**
- `src/api/http_server.rs` (modify)
  ```rust
  #[derive(Clone)]
  pub struct AppState {
      // ... existing fields ...
      pub vision_model_manager: Arc<RwLock<Option<Arc<VisionModelManager>>>>,
  }
  ```

---

### Sub-phase 6.2: Register Routes

**Goal**: Add OCR and describe-image routes to HTTP server

#### Tasks
- [ ] Add `/v1/ocr` route to create_app()
- [ ] Add `/v1/describe-image` route to create_app()
- [ ] Update GET /v1/models to include vision models

**Implementation Files:**
- `src/api/http_server.rs` (modify)
  ```rust
  pub fn create_app(state: Arc<AppState>) -> Router {
      Router::new()
          // ... existing routes ...
          .route("/v1/ocr", post(ocr_handler))
          .route("/v1/describe-image", post(describe_image_handler))
          // ...
  }
  ```

---

### Sub-phase 6.3: Model Initialization

**Goal**: Initialize VisionModelManager in main.rs

#### Tasks
- [ ] Add vision model initialization to main.rs
- [ ] Add environment variables for model paths
- [ ] Handle missing model directories gracefully
- [ ] Log model availability at startup

**Implementation Files:**
- `src/main.rs` (modify)
  ```rust
  // Initialize Vision Model Manager (optional)
  println!("Initializing vision model manager...");

  let ocr_model_dir = std::env::var("OCR_MODEL_PATH")
      .unwrap_or_else(|_| "./models/paddleocr-onnx".to_string());
  let florence_model_dir = std::env::var("FLORENCE_MODEL_PATH")
      .unwrap_or_else(|_| "./models/florence-2-onnx".to_string());

  match VisionModelManager::new(
      Some(&ocr_model_dir),
      Some(&florence_model_dir),
  ).await {
      Ok(manager) => {
          let models = manager.list_models();
          println!("Vision models loaded: {:?}", models);
          api_server.set_vision_model_manager(Arc::new(manager)).await;
      }
      Err(e) => {
          println!("Vision models not available: {}", e);
          println!("/v1/ocr and /v1/describe-image will return 503");
      }
  }
  ```

---

## Phase 7: Documentation (1 hour)

### Sub-phase 7.1: Update API Documentation

**Goal**: Document new endpoints in API.md

#### Tasks
- [ ] Add POST /v1/ocr documentation
- [ ] Add POST /v1/describe-image documentation
- [ ] Add request/response examples
- [ ] Add error codes documentation

**Implementation Files:**
- `docs/API.md` (modify)

---

### Sub-phase 7.2: Update Version

**Goal**: Update version information

#### Tasks
- [ ] Update VERSION file to `8.6.0-image-processing`
- [ ] Update src/version.rs with new version constants
- [ ] Add new features to FEATURES array
- [ ] Update BREAKING_CHANGES array

**Implementation Files:**
- `VERSION`
- `src/version.rs`
  ```rust
  pub const VERSION: &str = "v8.6.0-image-processing-2025-MM-DD";
  pub const VERSION_NUMBER: &str = "8.6.0";
  pub const VERSION_PATCH: u32 = 0;
  pub const VERSION_MINOR: u32 = 6;

  pub const FEATURES: &[&str] = &[
      // ... existing features ...
      "cpu-ocr",
      "florence-vision",
      "image-to-text",
      "multipart-upload",
  ];
  ```

---

## Environment Variables

```bash
# Vision Model Paths (optional - defaults shown)
OCR_MODEL_PATH=./models/paddleocr-onnx
FLORENCE_MODEL_PATH=./models/florence-2-onnx

# Existing environment variables unchanged
```

---

## Performance Targets

| Operation | Target | RAM Usage |
|-----------|--------|-----------|
| OCR (640x640) | <3s | ~2GB |
| Florence description (768x768) | <5s | ~4GB |
| Image preprocessing | <100ms | <100MB |

---

## Test Summary

| Phase | Test File | Test Count |
|-------|-----------|------------|
| 1.3 | test_ocr_request.rs | ~10 |
| 1.3 | test_ocr_response.rs | ~5 |
| 1.3 | test_describe_image_request.rs | ~10 |
| 1.3 | test_describe_image_response.rs | ~5 |
| 2.1 | test_image_loading.rs | ~10 |
| 2.2 | test_ocr_preprocessing.rs | ~8 |
| 2.3 | test_florence_preprocessing.rs | ~8 |
| 3.1 | test_ocr_detection_model.rs | ~8 |
| 3.2 | test_ocr_recognition_model.rs | ~8 |
| 3.3 | test_ocr_pipeline.rs | ~10 |
| 4.1 | test_florence_encoder.rs | ~6 |
| 4.2 | test_florence_decoder.rs | ~8 |
| 4.3 | test_florence_pipeline.rs | ~10 |
| 5.1 | test_model_manager.rs | ~8 |
| 5.2 | test_ocr_endpoint.rs | ~12 |
| 5.3 | test_describe_image_endpoint.rs | ~12 |
| **Total** | | **~138 tests** |

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| PaddleOCR ONNX not available | Use alternative from HuggingFace or paddle2onnx conversion |
| Florence-2 ONNX conversion issues | Fall back to smaller model or use pre-converted from onnx-community |
| Memory pressure | Implement lazy loading, model eviction if needed |
| CPU performance | Limit concurrent requests, add request queuing |
| Image size attacks | Strict 10MB limit, dimension validation |
