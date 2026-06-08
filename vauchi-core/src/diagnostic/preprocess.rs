// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tier 1 image preprocessing pipeline for QR scanner benchmarking.
//!
//! Applies cheap filters that dramatically improve QR decode rates on
//! real camera frames: downscale, CLAHE, adaptive threshold, unsharp
//! mask, and sharpness gating.

use image::GrayImage;
use serde::{Deserialize, Serialize};

/// Configuration for the Tier 1 preprocessing pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreprocessConfig {
    /// Target width for downscaling (0 = no downscale).
    pub target_width: u32,
    /// CLAHE clip limit (typical: 2.0–3.0).
    pub clahe_clip_limit: f32,
    /// CLAHE tile grid size (typical: 8).
    pub clahe_tile_size: u32,
    /// Adaptive threshold window size in pixels (typical: 25).
    pub threshold_window: u32,
    /// Unsharp mask Gaussian blur sigma (typical: 1.0–2.0).
    pub unsharp_sigma: f32,
    /// Unsharp mask strength (typical: 0.5–1.0).
    pub unsharp_amount: f32,
    /// Laplacian variance below this → skip frame (0.0 = disabled).
    pub sharpness_threshold: f32,
    /// Apply CLAHE contrast enhancement (disable for already-clean images).
    pub apply_clahe: bool,
    /// Apply unsharp mask edge sharpening.
    pub apply_unsharp: bool,
    /// Apply adaptive thresholding as final step (produces binary image).
    /// Disable when feeding to rqrr which does its own binarization.
    pub apply_threshold: bool,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            target_width: 720,
            clahe_clip_limit: 2.5,
            clahe_tile_size: 8,
            threshold_window: 25,
            unsharp_sigma: 1.5,
            unsharp_amount: 0.7,
            sharpness_threshold: 100.0,
            apply_clahe: true,
            apply_unsharp: true,
            apply_threshold: false,
        }
    }
}

/// Result of preprocessing a single frame.
pub struct PreprocessResult {
    /// The preprocessed grayscale image.
    pub image: GrayImage,
    /// Whether the frame was skipped (below sharpness threshold).
    pub skipped: bool,
    /// Laplacian variance (sharpness metric).
    pub laplacian_variance: f32,
    /// Time spent on preprocessing in microseconds.
    pub preprocess_time_us: u64,
}

/// Run the full Tier 1 preprocessing pipeline on a grayscale frame.
pub fn preprocess_frame(img: GrayImage, config: &PreprocessConfig) -> PreprocessResult {
    let start = std::time::Instant::now();

    // 1. Sharpness gating (compute before any transforms)
    let laplacian_var = compute_laplacian_variance(&img);
    if config.sharpness_threshold > 0.0 && laplacian_var < config.sharpness_threshold {
        return PreprocessResult {
            image: img,
            skipped: true,
            laplacian_variance: laplacian_var,
            preprocess_time_us: start.elapsed().as_micros() as u64,
        };
    }

    let img = if config.target_width > 0 && img.width() > config.target_width {
        downscale_luma(img, config.target_width)
    } else {
        img
    };

    let mut img = if config.apply_clahe {
        apply_clahe(img, config.clahe_clip_limit, config.clahe_tile_size)
    } else {
        img
    };

    if config.apply_unsharp {
        apply_unsharp_mask(&mut img, config.unsharp_sigma, config.unsharp_amount);
    }

    // 5. Adaptive threshold (optional — rqrr does its own binarization)
    let img = if config.apply_threshold {
        apply_adaptive_threshold(&img, config.threshold_window)
    } else {
        img
    };

    PreprocessResult {
        image: img,
        skipped: false,
        laplacian_variance: laplacian_var,
        preprocess_time_us: start.elapsed().as_micros() as u64,
    }
}

/// Compute the variance of the Laplacian as a sharpness metric.
///
/// Higher values indicate sharper images. Motion-blurred frames score low.
pub fn compute_laplacian_variance(img: &GrayImage) -> f32 {
    let (w, h) = img.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }

    let mut sum = 0i64;
    let mut sum_sq = 0i64;
    let mut count = 0u64;

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            // 3x3 Laplacian kernel: [0,-1,0; -1,4,-1; 0,-1,0]
            let center = img.get_pixel(x, y)[0] as i32;
            let top = img.get_pixel(x, y - 1)[0] as i32;
            let bottom = img.get_pixel(x, y + 1)[0] as i32;
            let left = img.get_pixel(x - 1, y)[0] as i32;
            let right = img.get_pixel(x + 1, y)[0] as i32;
            let lap = 4 * center - top - bottom - left - right;
            sum += lap as i64;
            sum_sq += (lap as i64) * (lap as i64);
            count += 1;
        }
    }

    if count == 0 {
        return 0.0;
    }

    let mean = sum as f64 / count as f64;
    let variance = (sum_sq as f64 / count as f64) - (mean * mean);
    variance as f32
}

/// Downscale a grayscale image to the target width, preserving aspect ratio.
fn downscale_luma(img: GrayImage, target_width: u32) -> GrayImage {
    use fast_image_resize::images::Image;
    use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

    let (src_w, src_h) = img.dimensions();
    let scale = target_width as f64 / src_w as f64;
    let dst_h = (src_h as f64 * scale).round() as u32;
    let dst_w = target_width;

    // Guard against pathological aspect ratios (e.g. 4000x2 → dst_h=0)
    if dst_w == 0 || dst_h == 0 {
        return img;
    }

    let src_image = Image::from_vec_u8(src_w, src_h, img.into_raw(), PixelType::U8)
        .expect("source image creation");

    let mut dst_image = Image::new(dst_w, dst_h, PixelType::U8);

    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    let mut resizer = Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, &options)
        .expect("resize");

    GrayImage::from_raw(dst_w, dst_h, dst_image.into_vec()).expect("output image creation")
}

/// Apply Contrast Limited Adaptive Histogram Equalization (CLAHE).
///
/// Inline implementation (~80 lines) to avoid adding a crate dependency
/// for a single function. Divides the image into tiles, computes a
/// clipped histogram per tile, and uses bilinear interpolation at tile
/// borders.
fn apply_clahe(img: GrayImage, clip_limit: f32, tile_size: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    let tw = tile_size.max(1);
    let th = tile_size.max(1);
    let nx = w.div_ceil(tw);
    let ny = h.div_ceil(th);

    // Compute clipped + redistributed CDF per tile
    let mut tile_cdfs: Vec<[f32; 256]> = Vec::with_capacity((nx * ny) as usize);

    for ty in 0..ny {
        for tx in 0..nx {
            let x0 = tx * tw;
            let y0 = ty * th;
            let x1 = (x0 + tw).min(w);
            let y1 = (y0 + th).min(h);
            let pixel_count = ((x1 - x0) * (y1 - y0)) as f32;

            let mut hist = [0u32; 256];
            for y in y0..y1 {
                for x in x0..x1 {
                    hist[img.get_pixel(x, y)[0] as usize] += 1;
                }
            }

            let clip = (clip_limit * pixel_count / 256.0) as u32;
            let mut excess = 0u32;
            for h in hist.iter_mut() {
                if *h > clip {
                    excess += *h - clip;
                    *h = clip;
                }
            }
            let redist = excess / 256;
            for h in hist.iter_mut() {
                *h += redist;
            }

            // Build CDF (normalized to 0..255)
            let mut cdf = [0f32; 256];
            let mut cumulative = 0u32;
            for i in 0..256 {
                cumulative += hist[i];
                cdf[i] = cumulative as f32 / pixel_count;
            }

            tile_cdfs.push(cdf);
        }
    }

    // Interpolate: for each pixel, blend the CDFs of the 4 nearest tile centers
    let mut out = GrayImage::new(w, h);
    let half_tw = tw as f32 / 2.0;
    let half_th = th as f32 / 2.0;

    for y in 0..h {
        for x in 0..w {
            let val = img.get_pixel(x, y)[0] as usize;

            let fx = (x as f32 - half_tw) / tw as f32;
            let fy = (y as f32 - half_th) / th as f32;
            let tx0 = (fx.floor() as i32).clamp(0, nx as i32 - 1) as u32;
            let ty0 = (fy.floor() as i32).clamp(0, ny as i32 - 1) as u32;
            let tx1 = (tx0 + 1).min(nx - 1);
            let ty1 = (ty0 + 1).min(ny - 1);

            let ax = (fx - fx.floor()).clamp(0.0, 1.0);
            let ay = (fy - fy.floor()).clamp(0.0, 1.0);

            let c00 = tile_cdfs[(ty0 * nx + tx0) as usize][val];
            let c10 = tile_cdfs[(ty0 * nx + tx1) as usize][val];
            let c01 = tile_cdfs[(ty1 * nx + tx0) as usize][val];
            let c11 = tile_cdfs[(ty1 * nx + tx1) as usize][val];

            let top = c00 * (1.0 - ax) + c10 * ax;
            let bot = c01 * (1.0 - ax) + c11 * ax;
            let result = top * (1.0 - ay) + bot * ay;

            out.put_pixel(
                x,
                y,
                image::Luma([(result * 255.0).clamp(0.0, 255.0) as u8]),
            );
        }
    }

    out
}

/// Apply unsharp mask: sharpen edges by subtracting a blurred version.
fn apply_unsharp_mask(img: &mut GrayImage, sigma: f32, amount: f32) {
    let blurred = imageproc::filter::gaussian_blur_f32(img, sigma);
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            let orig = img.get_pixel(x, y)[0] as f32;
            let blur = blurred.get_pixel(x, y)[0] as f32;
            let sharp = orig + amount * (orig - blur);
            img.put_pixel(x, y, image::Luma([sharp.clamp(0.0, 255.0) as u8]));
        }
    }
}

/// Apply Sauvola adaptive thresholding.
fn apply_adaptive_threshold(img: &GrayImage, window: u32) -> GrayImage {
    imageproc::contrast::adaptive_threshold(img, window)
}
