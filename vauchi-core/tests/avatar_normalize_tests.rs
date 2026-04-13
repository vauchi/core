// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for avatar normalization (ADR-042).
//!
//! Verifies that `normalize_avatar` converts PNG, JPEG, BMP, and WebP inputs
//! to WebP output within the MAX_AVATAR_SIZE budget.

use image::{ImageBuffer, Rgb, RgbImage};
use std::io::Cursor;
use vauchi_core::contact_card::{ContactCardError, MAX_AVATAR_SIZE, normalize_avatar};

/// Helper: encode an RgbImage to PNG bytes.
fn encode_png(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("PNG encoding should succeed");
    buf
}

/// Helper: encode an RgbImage to JPEG bytes.
fn encode_jpeg(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .expect("JPEG encoding should succeed");
    buf
}

/// Helper: encode an RgbImage to WebP bytes.
fn encode_webp(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::WebP)
        .expect("WebP encoding should succeed");
    buf
}

/// Helper: create a 1x1 pixel image with a specific color.
fn tiny_image() -> RgbImage {
    ImageBuffer::from_pixel(1, 1, Rgb([42u8, 128, 200]))
}

/// Helper: verify the output bytes start with the RIFF/WebP magic.
fn assert_webp_magic(data: &[u8]) {
    assert!(data.len() >= 12, "Output too short to be WebP");
    assert_eq!(&data[0..4], b"RIFF", "Missing RIFF header");
    assert_eq!(&data[8..12], b"WEBP", "Missing WEBP magic");
}

// @scenario: contact_card_management :: Normalize PNG avatar to WebP
#[test]
fn test_normalize_png_to_webp() {
    let png_bytes = encode_png(&tiny_image());

    let result = normalize_avatar(&png_bytes).expect("PNG normalization should succeed");

    assert_webp_magic(&result);
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "Output {} bytes exceeds MAX_AVATAR_SIZE {}",
        result.len(),
        MAX_AVATAR_SIZE
    );
}

// @scenario: contact_card_management :: Normalize JPEG avatar to WebP
#[test]
fn test_normalize_jpeg_to_webp() {
    let jpeg_bytes = encode_jpeg(&tiny_image());

    let result = normalize_avatar(&jpeg_bytes).expect("JPEG normalization should succeed");

    assert_webp_magic(&result);
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "Output {} bytes exceeds MAX_AVATAR_SIZE {}",
        result.len(),
        MAX_AVATAR_SIZE
    );
}

// @scenario: contact_card_management :: Normalize WebP avatar passthrough
#[test]
fn test_normalize_webp_passthrough() {
    let webp_bytes = encode_webp(&tiny_image());

    let result = normalize_avatar(&webp_bytes).expect("WebP normalization should succeed");

    assert_webp_magic(&result);
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "Output {} bytes exceeds MAX_AVATAR_SIZE {}",
        result.len(),
        MAX_AVATAR_SIZE
    );
}

// @scenario: contact_card_management :: Reject invalid avatar data
#[test]
fn test_normalize_invalid_data_rejected() {
    let garbage = b"this is not an image at all";

    let result = normalize_avatar(garbage);

    assert!(result.is_err(), "Garbage data should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, ContactCardError::AvatarInvalidFormat),
        "Expected AvatarInvalidFormat, got: {err}"
    );
}

// @scenario: contact_card_management :: Reject empty avatar data
#[test]
fn test_normalize_empty_data_rejected() {
    let result = normalize_avatar(b"");

    assert!(result.is_err(), "Empty data should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, ContactCardError::AvatarInvalidFormat),
        "Expected AvatarInvalidFormat, got: {err}"
    );
}

// @scenario: contact_card_management :: Resize large avatar image
#[test]
fn test_normalize_large_image_resized() {
    // Create an 800x800 image with varying pixel data to prevent trivial compression.
    let img: RgbImage = ImageBuffer::from_fn(800, 800, |x, y| {
        Rgb([
            ((x * 7 + y * 13) % 256) as u8,
            ((x * 11 + y * 3) % 256) as u8,
            ((x * 5 + y * 17) % 256) as u8,
        ])
    });
    let png_bytes = encode_png(&img);

    let result = normalize_avatar(&png_bytes).expect("Large image normalization should succeed");

    assert_webp_magic(&result);
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "Output {} bytes exceeds MAX_AVATAR_SIZE {}",
        result.len(),
        MAX_AVATAR_SIZE
    );
}

// @scenario: contact_card_management :: Very large avatar always within size limit
#[test]
fn test_normalize_output_always_within_limit() {
    // Create a 2000x2000 image with high-entropy pixel data.
    let img: RgbImage = ImageBuffer::from_fn(2000, 2000, |x, y| {
        Rgb([
            ((x.wrapping_mul(31).wrapping_add(y.wrapping_mul(97))) % 256) as u8,
            ((x.wrapping_mul(53).wrapping_add(y.wrapping_mul(41))) % 256) as u8,
            ((x.wrapping_mul(79).wrapping_add(y.wrapping_mul(67))) % 256) as u8,
        ])
    });
    let png_bytes = encode_png(&img);

    let result =
        normalize_avatar(&png_bytes).expect("Very large image normalization should succeed");

    assert_webp_magic(&result);
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "Output {} bytes exceeds MAX_AVATAR_SIZE {}",
        result.len(),
        MAX_AVATAR_SIZE
    );
}
