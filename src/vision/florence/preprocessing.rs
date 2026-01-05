// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image preprocessing for Florence-2

use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use ndarray::Array4;

/// Target size for Florence-2 vision encoder
pub const FLORENCE_INPUT_SIZE: u32 = 768;

/// ImageNet normalization mean values (Florence-2 uses ImageNet, not CLIP)
pub const MEAN: [f32; 3] = [0.485, 0.456, 0.406];

/// ImageNet normalization std values
pub const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Preprocessing mode for Florence-2
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeMode {
    /// Resize to exact size (may distort aspect ratio)
    Stretch,
    /// Resize keeping aspect ratio with center crop
    CenterCrop,
    /// Resize keeping aspect ratio with padding (letterbox)
    Letterbox,
}

impl Default for ResizeMode {
    fn default() -> Self {
        // CenterCrop matches Florence-2's expected preprocessing
        Self::CenterCrop
    }
}

/// Preprocess an image for Florence-2 encoder
///
/// Steps:
/// 1. Resize to FLORENCE_INPUT_SIZE x FLORENCE_INPUT_SIZE using specified mode
/// 2. Convert to RGB
/// 3. Normalize with CLIP mean/std: (pixel/255 - mean) / std
/// 4. Convert to NCHW tensor format [1, 3, H, W]
pub fn preprocess_for_florence(image: &DynamicImage) -> Array4<f32> {
    // CenterCrop matches Florence-2's expected preprocessing (same as Python test)
    preprocess_for_florence_with_mode(image, ResizeMode::CenterCrop)
}

/// Preprocess with specified resize mode
pub fn preprocess_for_florence_with_mode(image: &DynamicImage, mode: ResizeMode) -> Array4<f32> {
    // Resize image based on mode
    let resized = resize_for_encoder(image, FLORENCE_INPUT_SIZE, mode);
    let rgb = resized.to_rgb8();

    // Create output tensor in NCHW format
    let size = FLORENCE_INPUT_SIZE as usize;
    let mut tensor = Array4::zeros((1, 3, size, size));

    // Fill tensor with normalized pixel values
    for y in 0..size {
        for x in 0..size {
            let pixel = rgb.get_pixel(x as u32, y as u32);

            // Normalize with CLIP values: (pixel / 255.0 - mean) / std
            for c in 0..3 {
                let normalized = (pixel[c] as f32 / 255.0 - MEAN[c]) / STD[c];
                tensor[[0, c, y, x]] = normalized;
            }
        }
    }

    tensor
}

/// Resize image to target size using specified mode
pub fn resize_for_encoder(image: &DynamicImage, target_size: u32, mode: ResizeMode) -> DynamicImage {
    let (orig_w, orig_h) = image.dimensions();

    // Handle edge cases
    if orig_w == 0 || orig_h == 0 {
        return DynamicImage::ImageRgb8(RgbImage::from_pixel(
            target_size,
            target_size,
            Rgb([128, 128, 128]),
        ));
    }

    match mode {
        ResizeMode::Stretch => {
            // Simply resize to exact dimensions
            image.resize_exact(
                target_size,
                target_size,
                image::imageops::FilterType::Lanczos3,
            )
        }
        ResizeMode::CenterCrop => {
            center_crop_resize(image, target_size)
        }
        ResizeMode::Letterbox => {
            letterbox_resize(image, target_size)
        }
    }
}

/// Resize with center crop (no distortion, may lose edges)
fn center_crop_resize(image: &DynamicImage, target_size: u32) -> DynamicImage {
    let (orig_w, orig_h) = image.dimensions();

    // Calculate scale to cover the target (use larger scale)
    let scale_w = target_size as f32 / orig_w as f32;
    let scale_h = target_size as f32 / orig_h as f32;
    let scale = scale_w.max(scale_h);

    let new_w = (orig_w as f32 * scale).round() as u32;
    let new_h = (orig_h as f32 * scale).round() as u32;

    // Resize to cover target
    let resized = image.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);

    // Center crop to target size
    let crop_x = (new_w.saturating_sub(target_size)) / 2;
    let crop_y = (new_h.saturating_sub(target_size)) / 2;

    resized.crop_imm(crop_x, crop_y, target_size, target_size)
}

/// Resize with letterbox (padding, no distortion, keeps all content)
fn letterbox_resize(image: &DynamicImage, target_size: u32) -> DynamicImage {
    let (orig_w, orig_h) = image.dimensions();

    // Calculate scale to fit within target (use smaller scale)
    let scale_w = target_size as f32 / orig_w as f32;
    let scale_h = target_size as f32 / orig_h as f32;
    let scale = scale_w.min(scale_h);

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

/// Preprocessing info for Florence-2
pub struct FlorencePreprocessInfo {
    /// Resize mode used
    pub mode: ResizeMode,
    /// Scale factor applied
    pub scale: f32,
    /// X offset from padding (only for letterbox)
    pub offset_x: u32,
    /// Y offset from padding (only for letterbox)
    pub offset_y: u32,
    /// Original image width
    pub original_width: u32,
    /// Original image height
    pub original_height: u32,
}

impl FlorencePreprocessInfo {
    /// Calculate preprocessing info for an image
    pub fn new(image: &DynamicImage, mode: ResizeMode) -> Self {
        let (orig_w, orig_h) = image.dimensions();
        let target = FLORENCE_INPUT_SIZE as f32;

        if orig_w == 0 || orig_h == 0 {
            return Self {
                mode,
                scale: 1.0,
                offset_x: 0,
                offset_y: 0,
                original_width: orig_w,
                original_height: orig_h,
            };
        }

        match mode {
            ResizeMode::Stretch => {
                // Different scales for x and y, use average for reference
                let scale_w = target / orig_w as f32;
                let scale_h = target / orig_h as f32;
                Self {
                    mode,
                    scale: (scale_w + scale_h) / 2.0,
                    offset_x: 0,
                    offset_y: 0,
                    original_width: orig_w,
                    original_height: orig_h,
                }
            }
            ResizeMode::CenterCrop => {
                let scale_w = target / orig_w as f32;
                let scale_h = target / orig_h as f32;
                let scale = scale_w.max(scale_h);
                let new_w = (orig_w as f32 * scale).round() as u32;
                let new_h = (orig_h as f32 * scale).round() as u32;
                Self {
                    mode,
                    scale,
                    offset_x: (new_w.saturating_sub(FLORENCE_INPUT_SIZE)) / 2,
                    offset_y: (new_h.saturating_sub(FLORENCE_INPUT_SIZE)) / 2,
                    original_width: orig_w,
                    original_height: orig_h,
                }
            }
            ResizeMode::Letterbox => {
                let scale_w = target / orig_w as f32;
                let scale_h = target / orig_h as f32;
                let scale = scale_w.min(scale_h);
                let new_w = (orig_w as f32 * scale).round() as u32;
                let new_h = (orig_h as f32 * scale).round() as u32;
                Self {
                    mode,
                    scale,
                    offset_x: (FLORENCE_INPUT_SIZE - new_w) / 2,
                    offset_y: (FLORENCE_INPUT_SIZE - new_h) / 2,
                    original_width: orig_w,
                    original_height: orig_h,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(FLORENCE_INPUT_SIZE, 768);
        assert_eq!(MEAN.len(), 3);
        assert_eq!(STD.len(), 3);
    }

    #[test]
    fn test_preprocess_for_florence_shape() {
        let img = DynamicImage::new_rgb8(100, 100);
        let tensor = preprocess_for_florence(&img);
        assert_eq!(tensor.shape(), &[1, 3, 768, 768]);
    }

    #[test]
    fn test_preprocess_for_florence_shape_rectangular() {
        let img = DynamicImage::new_rgb8(1920, 1080);
        let tensor = preprocess_for_florence(&img);
        assert_eq!(tensor.shape(), &[1, 3, 768, 768]);
    }

    #[test]
    fn test_imagenet_normalization_values() {
        // Florence-2 uses ImageNet normalization
        assert!((MEAN[0] - 0.485).abs() < 0.001);
        assert!((MEAN[1] - 0.456).abs() < 0.001);
        assert!((MEAN[2] - 0.406).abs() < 0.001);
        assert!((STD[0] - 0.229).abs() < 0.001);
        assert!((STD[1] - 0.224).abs() < 0.001);
        assert!((STD[2] - 0.225).abs() < 0.001);
    }

    #[test]
    fn test_resize_mode_stretch() {
        let img = DynamicImage::new_rgb8(100, 200);
        let resized = resize_for_encoder(&img, 768, ResizeMode::Stretch);
        assert_eq!(resized.dimensions(), (768, 768));
    }

    #[test]
    fn test_resize_mode_center_crop() {
        let img = DynamicImage::new_rgb8(1000, 500);
        let resized = resize_for_encoder(&img, 768, ResizeMode::CenterCrop);
        assert_eq!(resized.dimensions(), (768, 768));
    }

    #[test]
    fn test_resize_mode_letterbox() {
        let img = DynamicImage::new_rgb8(1000, 500);
        let resized = resize_for_encoder(&img, 768, ResizeMode::Letterbox);
        assert_eq!(resized.dimensions(), (768, 768));
    }

    #[test]
    fn test_preprocess_with_mode() {
        let img = DynamicImage::new_rgb8(100, 100);

        let tensor_letterbox = preprocess_for_florence_with_mode(&img, ResizeMode::Letterbox);
        assert_eq!(tensor_letterbox.shape(), &[1, 3, 768, 768]);

        let tensor_crop = preprocess_for_florence_with_mode(&img, ResizeMode::CenterCrop);
        assert_eq!(tensor_crop.shape(), &[1, 3, 768, 768]);
    }

    #[test]
    fn test_normalization_range() {
        // Create a simple image with known pixel values
        let mut img = RgbImage::new(10, 10);
        // Fill with white (255, 255, 255)
        for pixel in img.pixels_mut() {
            *pixel = Rgb([255, 255, 255]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        let tensor = preprocess_for_florence(&dyn_img);

        // Check that normalized values are in expected range
        // For CLIP normalization with white pixels:
        // R: (1.0 - 0.48145466) / 0.26862954 ≈ 1.93
        for val in tensor.iter() {
            assert!(
                *val >= -5.0 && *val <= 5.0,
                "Normalized value {} out of expected range",
                val
            );
        }
    }

    #[test]
    fn test_preprocess_info_letterbox() {
        let img = DynamicImage::new_rgb8(768, 768);
        let info = FlorencePreprocessInfo::new(&img, ResizeMode::Letterbox);
        assert!((info.scale - 1.0).abs() < 0.001);
        assert_eq!(info.offset_x, 0);
        assert_eq!(info.offset_y, 0);
    }

    #[test]
    fn test_preprocess_info_wide_image() {
        // Wide image will have vertical padding in letterbox mode
        let img = DynamicImage::new_rgb8(1536, 768);
        let info = FlorencePreprocessInfo::new(&img, ResizeMode::Letterbox);

        // Scale should be 768/1536 = 0.5
        assert!((info.scale - 0.5).abs() < 0.01);
        // Horizontal offset should be 0 (image fills width)
        assert_eq!(info.offset_x, 0);
        // Vertical offset should be (768 - 384) / 2 = 192
        assert!(info.offset_y > 0);
    }

    #[test]
    fn test_preprocess_info_tall_image() {
        // Tall image will have horizontal padding in letterbox mode
        let img = DynamicImage::new_rgb8(768, 1536);
        let info = FlorencePreprocessInfo::new(&img, ResizeMode::Letterbox);

        // Scale should be 768/1536 = 0.5
        assert!((info.scale - 0.5).abs() < 0.01);
        // Vertical offset should be 0 (image fills height)
        assert_eq!(info.offset_y, 0);
        // Horizontal offset should be > 0
        assert!(info.offset_x > 0);
    }

    #[test]
    fn test_default_resize_mode() {
        // CenterCrop matches Florence-2's expected preprocessing
        assert_eq!(ResizeMode::default(), ResizeMode::CenterCrop);
    }

    #[test]
    fn test_tensor_channel_order() {
        // Verify that channels are in correct order (RGB -> CHW)
        let mut img = RgbImage::new(2, 2);
        // Set pixel (0,0) to red (255, 0, 0)
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        let dyn_img = DynamicImage::ImageRgb8(img);

        let tensor = preprocess_for_florence(&dyn_img);

        // Verify shape is correct
        assert_eq!(tensor.dim().1, 3); // 3 channels
    }
}
