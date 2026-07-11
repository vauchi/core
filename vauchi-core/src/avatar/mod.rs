// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Avatar generation and normalization (ADR-042).
//!
//! Core generates avatar images (initials circle, Mandelbrot fractal) so
//! all platforms produce identical output. Frontends never generate avatars
//! locally — they display the WebP bytes returned by these functions.

pub use crate::contact_card::{MAX_AVATAR_SIZE, normalize_avatar};

/// Generate a solid-color circle avatar image.
///
/// Returns WebP bytes <= `MAX_AVATAR_SIZE`. Frontends overlay initials
/// text from the `ImageCircle` component's `initials` field — this
/// function only produces the colored circle background.
pub fn generate_initials_avatar(bg_color: [u8; 3], size: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(size, size);
    let center = size as f32 / 2.0;
    let radius_sq = center * center;

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let dx = x as f32 - center + 0.5;
        let dy = y as f32 - center + 0.5;
        if dx * dx + dy * dy <= radius_sq {
            *pixel = image::Rgba([bg_color[0], bg_color[1], bg_color[2], 255]);
        } else {
            *pixel = image::Rgba([0, 0, 0, 0]);
        }
    }

    encode_to_webp(&img)
}

/// Generate a Mandelbrot fractal avatar image.
///
/// Different `seed` values produce different views of the Mandelbrot set,
/// giving users variety. Returns WebP bytes <= `MAX_AVATAR_SIZE`.
pub fn generate_mandelbrot_avatar(seed: u64, size: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(size, size);
    let center_x = -0.5 + (seed % 10) as f64 * 0.05;
    let center_y = (seed / 10 % 10) as f64 * 0.05;
    let zoom = 2.5;
    let max_iter = 100u32;
    let center = size as f32 / 2.0;
    let radius_sq = center * center;

    for (px, py, pixel) in img.enumerate_pixels_mut() {
        let dx = px as f32 - center + 0.5;
        let dy = py as f32 - center + 0.5;
        if dx * dx + dy * dy > radius_sq {
            *pixel = image::Rgba([0, 0, 0, 0]);
            continue;
        }
        let x0 = center_x + (px as f64 / size as f64 - 0.5) * zoom;
        let y0 = center_y + (py as f64 / size as f64 - 0.5) * zoom;
        let mut x = 0.0_f64;
        let mut y = 0.0_f64;
        let mut iter = 0u32;
        while x * x + y * y <= 4.0 && iter < max_iter {
            let xtemp = x * x - y * y + x0;
            y = 2.0 * x * y + y0;
            x = xtemp;
            iter += 1;
        }
        if iter == max_iter {
            *pixel = image::Rgba([0, 0, 0, 255]);
        } else {
            let t = iter as f64 / max_iter as f64;
            *pixel = image::Rgba([
                (t * 9.0 * 255.0).min(255.0) as u8,
                (t * 3.0 * 255.0).min(255.0) as u8,
                (t * 255.0).min(255.0) as u8,
                255,
            ]);
        }
    }

    encode_to_webp(&img)
}

/// Encode an RGBA image to WebP, normalizing through the avatar pipeline.
fn encode_to_webp(img: &image::RgbaImage) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .expect("PNG encoding should never fail for in-memory images");
    normalize_avatar(&buf.into_inner())
        .expect("generated image should always normalize successfully")
}
