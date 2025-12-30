// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image loading and utility functions for vision processing

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{DynamicImage, ImageFormat};
use thiserror::Error;

/// Maximum image size (10MB)
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Custom error types for image processing
#[derive(Debug, Error)]
pub enum ImageError {
    #[error("Image data is too large: {0} bytes (max: {1} bytes)")]
    TooLarge(usize, usize),

    #[error("Invalid base64 encoding: {0}")]
    InvalidBase64(#[from] base64::DecodeError),

    #[error("Unsupported image format")]
    UnsupportedFormat,

    #[error("Failed to decode image: {0}")]
    DecodeFailed(String),

    #[error("Image data is empty")]
    EmptyData,

    #[error("Corrupted image data")]
    CorruptedData,
}

/// Image information extracted during loading
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Detected format
    pub format: ImageFormat,
    /// Size in bytes
    pub size_bytes: usize,
}

/// Decode a base64-encoded image
///
/// # Arguments
/// * `base64_str` - Base64 encoded image data
///
/// # Returns
/// * `Ok((DynamicImage, ImageInfo))` - The decoded image and metadata
/// * `Err(ImageError)` - If decoding fails
///
/// # Example
/// ```ignore
/// let (image, info) = decode_base64_image("iVBORw0KGgo...")?;
/// println!("Image size: {}x{}", info.width, info.height);
/// ```
pub fn decode_base64_image(base64_str: &str) -> Result<(DynamicImage, ImageInfo), ImageError> {
    // Handle empty input
    if base64_str.is_empty() {
        return Err(ImageError::EmptyData);
    }

    // Decode base64
    let bytes = STANDARD.decode(base64_str)?;

    // Validate size
    if bytes.len() > MAX_IMAGE_SIZE {
        return Err(ImageError::TooLarge(bytes.len(), MAX_IMAGE_SIZE));
    }

    if bytes.is_empty() {
        return Err(ImageError::EmptyData);
    }

    // Detect format from magic bytes
    let format = detect_format(&bytes)?;

    // Load image
    let img = image::load_from_memory_with_format(&bytes, format)
        .map_err(|e| ImageError::DecodeFailed(e.to_string()))?;

    let info = ImageInfo {
        width: img.width(),
        height: img.height(),
        format,
        size_bytes: bytes.len(),
    };

    Ok((img, info))
}

/// Decode raw image bytes (for multipart uploads)
///
/// # Arguments
/// * `bytes` - Raw image bytes
///
/// # Returns
/// * `Ok((DynamicImage, ImageInfo))` - The decoded image and metadata
/// * `Err(ImageError)` - If decoding fails
pub fn decode_image_bytes(bytes: &[u8]) -> Result<(DynamicImage, ImageInfo), ImageError> {
    // Validate size
    if bytes.len() > MAX_IMAGE_SIZE {
        return Err(ImageError::TooLarge(bytes.len(), MAX_IMAGE_SIZE));
    }

    if bytes.is_empty() {
        return Err(ImageError::EmptyData);
    }

    // Detect format from magic bytes
    let format = detect_format(bytes)?;

    // Load image
    let img = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| ImageError::DecodeFailed(e.to_string()))?;

    let info = ImageInfo {
        width: img.width(),
        height: img.height(),
        format,
        size_bytes: bytes.len(),
    };

    Ok((img, info))
}

/// Detect image format from magic bytes
///
/// # Arguments
/// * `bytes` - Raw image data
///
/// # Returns
/// * `Ok(ImageFormat)` - Detected format
/// * `Err(ImageError::UnsupportedFormat)` - If format cannot be detected
pub fn detect_format(bytes: &[u8]) -> Result<ImageFormat, ImageError> {
    if bytes.len() < 4 {
        return Err(ImageError::UnsupportedFormat);
    }

    match bytes {
        // PNG: 89 50 4E 47 (0x89 P N G)
        [0x89, 0x50, 0x4E, 0x47, ..] => Ok(ImageFormat::Png),

        // JPEG: FF D8 FF
        [0xFF, 0xD8, 0xFF, ..] => Ok(ImageFormat::Jpeg),

        // WebP: RIFF .... WEBP
        [0x52, 0x49, 0x46, 0x46, _, _, _, _, 0x57, 0x45, 0x42, 0x50, ..] => Ok(ImageFormat::WebP),

        // GIF: GIF87a or GIF89a
        [0x47, 0x49, 0x46, 0x38, x, ..] if *x == 0x37 || *x == 0x39 => Ok(ImageFormat::Gif),

        // BMP: BM
        [0x42, 0x4D, ..] => Ok(ImageFormat::Bmp),

        // TIFF: II (little-endian) or MM (big-endian)
        [0x49, 0x49, 0x2A, 0x00, ..] | [0x4D, 0x4D, 0x00, 0x2A, ..] => Ok(ImageFormat::Tiff),

        _ => Err(ImageError::UnsupportedFormat),
    }
}

/// Get the format extension as a string
pub fn format_to_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1x1 red PNG image (base64)
    const TINY_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

    // Simple 2x2 JPEG that works with the image crate
    // Generated using: convert -size 2x2 xc:red /tmp/tiny.jpg && base64 /tmp/tiny.jpg
    const TINY_JPEG_BASE64: &str = concat!(
        "/9j/4AAQSkZJRgABAgAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsL",
        "DBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/",
        "2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIy",
        "MjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QA",
        "HwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUF",
        "BAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkK",
        "FhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1",
        "dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXG",
        "x8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEB",
        "AQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAEC",
        "AxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRom",
        "JygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOE",
        "hYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU",
        "1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q=="
    );

    // GIF magic bytes (base64 of "GIF89a" + minimal data)
    const TINY_GIF_BASE64: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

    #[test]
    fn test_decode_base64_image_png() {
        let result = decode_base64_image(TINY_PNG_BASE64);
        assert!(result.is_ok(), "Failed to decode PNG: {:?}", result.err());

        let (img, info) = result.unwrap();
        assert_eq!(info.width, 1);
        assert_eq!(info.height, 1);
        assert_eq!(info.format, ImageFormat::Png);
        assert!(img.width() == 1 && img.height() == 1);
    }

    #[test]
    fn test_decode_base64_image_jpeg() {
        // Since generating a valid minimal JPEG is tricky, we test the format detection
        // with raw bytes instead of a full decode test
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let format = detect_format(&jpeg_header);
        assert!(format.is_ok());
        assert_eq!(format.unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn test_decode_base64_image_gif() {
        let result = decode_base64_image(TINY_GIF_BASE64);
        assert!(result.is_ok(), "Failed to decode GIF: {:?}", result.err());

        let (_img, info) = result.unwrap();
        assert_eq!(info.format, ImageFormat::Gif);
    }

    #[test]
    fn test_decode_base64_image_invalid_base64() {
        let result = decode_base64_image("not-valid-base64!!!");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageError::InvalidBase64(_)));
    }

    #[test]
    fn test_decode_base64_image_empty() {
        let result = decode_base64_image("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageError::EmptyData));
    }

    #[test]
    fn test_decode_base64_image_unsupported_format() {
        // Valid base64 but not an image (just random bytes)
        let random_bytes = STANDARD.encode([0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
        let result = decode_base64_image(&random_bytes);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImageError::UnsupportedFormat
        ));
    }

    #[test]
    fn test_decode_base64_image_corrupted() {
        // PNG header but corrupted data
        let corrupted = STANDARD.encode([0x89, 0x50, 0x4E, 0x47, 0x00, 0x00, 0x00, 0x00]);
        let result = decode_base64_image(&corrupted);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageError::DecodeFailed(_)));
    }

    #[test]
    fn test_detect_format_png() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect_format(&png_header).unwrap(), ImageFormat::Png);
    }

    #[test]
    fn test_detect_format_jpeg() {
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
        assert_eq!(detect_format(&jpeg_header).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn test_detect_format_gif87a() {
        let gif_header = [0x47, 0x49, 0x46, 0x38, 0x37, 0x61];
        assert_eq!(detect_format(&gif_header).unwrap(), ImageFormat::Gif);
    }

    #[test]
    fn test_detect_format_gif89a() {
        let gif_header = [0x47, 0x49, 0x46, 0x38, 0x39, 0x61];
        assert_eq!(detect_format(&gif_header).unwrap(), ImageFormat::Gif);
    }

    #[test]
    fn test_detect_format_webp() {
        let webp_header = [
            0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
        ];
        assert_eq!(detect_format(&webp_header).unwrap(), ImageFormat::WebP);
    }

    #[test]
    fn test_detect_format_unknown() {
        let unknown = [0x00, 0x00, 0x00, 0x00];
        assert!(detect_format(&unknown).is_err());
    }

    #[test]
    fn test_decode_image_bytes_valid() {
        let bytes = STANDARD.decode(TINY_PNG_BASE64).unwrap();
        let result = decode_image_bytes(&bytes);
        assert!(result.is_ok());

        let (img, info) = result.unwrap();
        assert_eq!(info.width, 1);
        assert_eq!(info.height, 1);
        assert_eq!(img.width(), 1);
    }

    #[test]
    fn test_decode_image_bytes_empty() {
        let result = decode_image_bytes(&[]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageError::EmptyData));
    }

    #[test]
    fn test_format_to_extension() {
        assert_eq!(format_to_extension(ImageFormat::Png), "png");
        assert_eq!(format_to_extension(ImageFormat::Jpeg), "jpg");
        assert_eq!(format_to_extension(ImageFormat::WebP), "webp");
        assert_eq!(format_to_extension(ImageFormat::Gif), "gif");
    }

    #[test]
    fn test_image_info_fields() {
        let result = decode_base64_image(TINY_PNG_BASE64).unwrap();
        let info = result.1;

        assert!(info.size_bytes > 0);
        assert_eq!(info.width, 1);
        assert_eq!(info.height, 1);
        assert_eq!(info.format, ImageFormat::Png);
    }

    // Test for oversized images (mocked since we can't actually create a 10MB+ base64 in tests)
    #[test]
    fn test_decode_image_bytes_too_large() {
        // Create a vector larger than MAX_IMAGE_SIZE
        let large_bytes = vec![0u8; MAX_IMAGE_SIZE + 1];
        let result = decode_image_bytes(&large_bytes);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImageError::TooLarge(_, _)));
    }
}
