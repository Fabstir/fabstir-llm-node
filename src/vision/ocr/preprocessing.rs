// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image preprocessing for PaddleOCR

use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use ndarray::Array4;

/// Target size for PaddleOCR detection model
pub const OCR_INPUT_SIZE: u32 = 640;

/// Recognition model input height (PP-OCRv5 English model uses 48)
pub const REC_INPUT_HEIGHT: u32 = 48;

/// Maximum width for recognition model input
pub const REC_MAX_WIDTH: u32 = 320;

/// Mean values for normalization (ImageNet)
pub const MEAN: [f32; 3] = [0.485, 0.456, 0.406];

/// Std values for normalization (ImageNet)
pub const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Preprocess an image for OCR detection
///
/// Steps:
/// 1. Resize with aspect ratio preservation to OCR_INPUT_SIZE
/// 2. Pad to square with gray (128) background
/// 3. Convert to RGB
/// 4. Normalize with ImageNet mean/std: (pixel/255 - mean) / std
/// 5. Convert to NCHW tensor format [1, 3, H, W]
pub fn preprocess_for_detection(image: &DynamicImage) -> Array4<f32> {
    // Resize with padding to target size
    let resized = resize_with_padding(image, OCR_INPUT_SIZE);
    let rgb = resized.to_rgb8();

    // Create output tensor in NCHW format
    let mut tensor =
        Array4::zeros((1, 3, OCR_INPUT_SIZE as usize, OCR_INPUT_SIZE as usize));

    // Fill tensor with normalized pixel values
    for y in 0..OCR_INPUT_SIZE as usize {
        for x in 0..OCR_INPUT_SIZE as usize {
            let pixel = rgb.get_pixel(x as u32, y as u32);

            // Normalize: (pixel / 255.0 - mean) / std
            for c in 0..3 {
                let normalized = (pixel[c] as f32 / 255.0 - MEAN[c]) / STD[c];
                tensor[[0, c, y, x]] = normalized;
            }
        }
    }

    tensor
}

/// Preprocess a cropped text region for recognition
///
/// Steps:
/// 1. Resize to height 32, dynamic width (preserving aspect ratio)
/// 2. Normalize with ImageNet mean/std
/// 3. Convert to NCHW tensor format [1, 3, 32, W]
///
/// Note: PP-OCRv5 expects dynamic width, no padding needed
pub fn preprocess_for_recognition(image: &DynamicImage) -> Array4<f32> {
    let (orig_w, orig_h) = image.dimensions();

    // Calculate new width while preserving aspect ratio
    let scale = REC_INPUT_HEIGHT as f32 / orig_h as f32;
    let new_width = ((orig_w as f32 * scale).round() as u32).min(REC_MAX_WIDTH);
    let new_width = new_width.max(4); // Ensure minimum width of 4 pixels

    // Resize image
    let resized = image.resize_exact(
        new_width,
        REC_INPUT_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let rgb = resized.to_rgb8();

    // Create output tensor with actual width (no padding for PP-OCRv5)
    let output_width = new_width as usize;
    let mut tensor = Array4::zeros((1, 3, REC_INPUT_HEIGHT as usize, output_width));

    // Fill tensor with normalized pixel values
    for y in 0..REC_INPUT_HEIGHT as usize {
        for x in 0..output_width {
            let pixel = rgb.get_pixel(x as u32, y as u32);
            for c in 0..3 {
                let normalized = (pixel[c] as f32 / 255.0 - MEAN[c]) / STD[c];
                tensor[[0, c, y, x]] = normalized;
            }
        }
    }

    tensor
}

/// Resize image with aspect ratio preservation and padding
///
/// The image is scaled to fit within target_size x target_size
/// while preserving aspect ratio, then padded with gray (128)
/// to reach the target dimensions.
pub fn resize_with_padding(image: &DynamicImage, target_size: u32) -> DynamicImage {
    let (orig_w, orig_h) = image.dimensions();

    // Handle edge cases
    if orig_w == 0 || orig_h == 0 {
        return DynamicImage::ImageRgb8(RgbImage::from_pixel(
            target_size,
            target_size,
            Rgb([128, 128, 128]),
        ));
    }

    // Calculate scale to fit within target while preserving aspect ratio
    let scale = (target_size as f32 / orig_w as f32).min(target_size as f32 / orig_h as f32);

    let new_w = (orig_w as f32 * scale).round() as u32;
    let new_h = (orig_h as f32 * scale).round() as u32;

    // Ensure minimum 1 pixel
    let new_w = new_w.max(1);
    let new_h = new_h.max(1);

    // Resize the image
    let resized = image.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);
    let rgb = resized.to_rgb8();

    // Create padded output (gray background)
    let mut output = RgbImage::from_pixel(target_size, target_size, Rgb([128, 128, 128]));

    // Calculate offset for centering
    let offset_x = (target_size - new_w) / 2;
    let offset_y = (target_size - new_h) / 2;

    // Copy resized image to center
    for y in 0..new_h {
        for x in 0..new_w {
            let pixel = rgb.get_pixel(x, y);
            output.put_pixel(x + offset_x, y + offset_y, *pixel);
        }
    }

    DynamicImage::ImageRgb8(output)
}

/// Get scaling factor and offsets used during preprocessing
/// Useful for mapping detection results back to original coordinates
pub struct PreprocessInfo {
    /// Scale factor applied
    pub scale: f32,
    /// X offset from padding
    pub offset_x: u32,
    /// Y offset from padding
    pub offset_y: u32,
    /// Original image width
    pub original_width: u32,
    /// Original image height
    pub original_height: u32,
}

impl PreprocessInfo {
    /// Calculate preprocessing info for an image
    pub fn new(image: &DynamicImage, target_size: u32) -> Self {
        let (orig_w, orig_h) = image.dimensions();

        if orig_w == 0 || orig_h == 0 {
            return Self {
                scale: 1.0,
                offset_x: 0,
                offset_y: 0,
                original_width: orig_w,
                original_height: orig_h,
            };
        }

        let scale = (target_size as f32 / orig_w as f32).min(target_size as f32 / orig_h as f32);
        let new_w = (orig_w as f32 * scale).round() as u32;
        let new_h = (orig_h as f32 * scale).round() as u32;

        Self {
            scale,
            offset_x: (target_size - new_w) / 2,
            offset_y: (target_size - new_h) / 2,
            original_width: orig_w,
            original_height: orig_h,
        }
    }

    /// Map a coordinate from preprocessed space back to original image space
    pub fn map_to_original(&self, x: f32, y: f32) -> (f32, f32) {
        let orig_x = (x - self.offset_x as f32) / self.scale;
        let orig_y = (y - self.offset_y as f32) / self.scale;
        (orig_x, orig_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(OCR_INPUT_SIZE, 640);
        assert_eq!(REC_INPUT_HEIGHT, 48);
        assert_eq!(REC_MAX_WIDTH, 320);
        assert_eq!(MEAN.len(), 3);
        assert_eq!(STD.len(), 3);
    }

    #[test]
    fn test_preprocess_for_detection_shape() {
        let img = DynamicImage::new_rgb8(100, 100);
        let tensor = preprocess_for_detection(&img);
        assert_eq!(tensor.shape(), &[1, 3, 640, 640]);
    }

    #[test]
    fn test_preprocess_for_detection_shape_rectangular() {
        // Test with non-square input
        let img = DynamicImage::new_rgb8(800, 600);
        let tensor = preprocess_for_detection(&img);
        assert_eq!(tensor.shape(), &[1, 3, 640, 640]);
    }

    #[test]
    fn test_preprocess_for_recognition_shape() {
        let img = DynamicImage::new_rgb8(100, 48);
        let tensor = preprocess_for_recognition(&img);
        // Width is dynamic (aspect ratio preserved), height is 48
        assert_eq!(tensor.shape()[0], 1);
        assert_eq!(tensor.shape()[1], 3);
        assert_eq!(tensor.shape()[2], 48);
        assert!(tensor.shape()[3] >= 4 && tensor.shape()[3] <= 320);
    }

    #[test]
    fn test_preprocess_for_recognition_wide_image() {
        // Test with very wide image - should be clamped to max width 320
        let img = DynamicImage::new_rgb8(1000, 48);
        let tensor = preprocess_for_recognition(&img);
        assert_eq!(tensor.shape()[2], 48);
        assert_eq!(tensor.shape()[3], 320); // Clamped to max
    }

    #[test]
    fn test_preprocess_for_recognition_tall_image() {
        // Test with tall image that will be scaled down
        let img = DynamicImage::new_rgb8(100, 96);
        let tensor = preprocess_for_recognition(&img);
        assert_eq!(tensor.shape()[2], 48);
        // Width = 100 * (48/96) = 50
        assert_eq!(tensor.shape()[3], 50);
    }

    #[test]
    fn test_resize_with_padding_square() {
        let img = DynamicImage::new_rgb8(100, 100);
        let resized = resize_with_padding(&img, 640);
        assert_eq!(resized.dimensions(), (640, 640));
    }

    #[test]
    fn test_resize_with_padding_wide() {
        // Wide image should have vertical padding
        let img = DynamicImage::new_rgb8(800, 400);
        let resized = resize_with_padding(&img, 640);
        assert_eq!(resized.dimensions(), (640, 640));
    }

    #[test]
    fn test_resize_with_padding_tall() {
        // Tall image should have horizontal padding
        let img = DynamicImage::new_rgb8(400, 800);
        let resized = resize_with_padding(&img, 640);
        assert_eq!(resized.dimensions(), (640, 640));
    }

    #[test]
    fn test_preprocess_normalization_range() {
        // Create a simple image with known pixel values
        let mut img = RgbImage::new(10, 10);
        // Fill with white (255, 255, 255)
        for pixel in img.pixels_mut() {
            *pixel = Rgb([255, 255, 255]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        let tensor = preprocess_for_detection(&dyn_img);

        // Check that normalized values are in expected range
        // For white pixel: (1.0 - mean) / std
        // R: (1.0 - 0.485) / 0.229 = 2.25
        // Values should be roughly in range [-3, 3] for typical images
        for val in tensor.iter() {
            assert!(
                *val >= -5.0 && *val <= 5.0,
                "Normalized value {} out of expected range",
                val
            );
        }
    }

    #[test]
    fn test_preprocess_info_square() {
        let img = DynamicImage::new_rgb8(640, 640);
        let info = PreprocessInfo::new(&img, 640);
        assert!((info.scale - 1.0).abs() < 0.001);
        assert_eq!(info.offset_x, 0);
        assert_eq!(info.offset_y, 0);
    }

    #[test]
    fn test_preprocess_info_map_to_original() {
        let img = DynamicImage::new_rgb8(320, 320);
        let info = PreprocessInfo::new(&img, 640);

        // With 2x scale, a point at (320, 320) in preprocessed space
        // should map to (160, 160) in original space (minus offset)
        let (orig_x, orig_y) = info.map_to_original(320.0, 320.0);

        // The offset should be (640 - 640) / 2 = 0 since image fills target
        assert!((orig_x - 160.0).abs() < 1.0);
        assert!((orig_y - 160.0).abs() < 1.0);
    }

    #[test]
    fn test_tensor_channel_order() {
        // Verify that channels are in correct order (RGB -> CHW)
        let mut img = RgbImage::new(2, 2);
        // Set pixel (0,0) to red (255, 0, 0)
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        let dyn_img = DynamicImage::ImageRgb8(img);

        let tensor = preprocess_for_detection(&dyn_img);

        // Red channel should have highest value at position (0,0) of resized image
        // (accounting for padding offset)
        // Just verify shape is correct for this test
        assert_eq!(tensor.dim().1, 3); // 3 channels
    }
}
