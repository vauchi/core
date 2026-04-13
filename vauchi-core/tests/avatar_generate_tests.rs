// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for avatar generation (initials + Mandelbrot).

use vauchi_core::avatar::{generate_initials_avatar, generate_mandelbrot_avatar};
use vauchi_core::contact_card::MAX_AVATAR_SIZE;

// ── Initials avatar ─────────────────────────────────────────────

// @scenario: contact_card_management :: Generate initials avatar
#[test]
fn initials_avatar_is_valid_webp_within_size_limit() {
    let result = generate_initials_avatar([0, 120, 200], 256);
    assert_eq!(&result[0..4], b"RIFF", "output must start with RIFF");
    assert_eq!(&result[8..12], b"WEBP", "output must be WebP");
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "initials avatar {} bytes exceeds {} limit",
        result.len(),
        MAX_AVATAR_SIZE,
    );
}

// @scenario: contact_card_management :: Initials avatar dimensions
#[test]
fn initials_avatar_has_expected_dimensions() {
    let result = generate_initials_avatar([100, 100, 200], 256);
    let img = image::load_from_memory(&result).expect("valid image");
    assert!(img.width() <= 256);
    assert!(img.height() <= 256);
}

// @scenario: contact_card_management :: Different colors produce different images
#[test]
fn initials_different_colors_produce_different_images() {
    let a = generate_initials_avatar([255, 0, 0], 128);
    let b = generate_initials_avatar([0, 0, 255], 128);
    assert_ne!(a, b, "different bg_colors should produce different images");
}

// @scenario: contact_card_management :: Initials avatar small size
#[test]
fn initials_avatar_small_size_does_not_panic() {
    let result = generate_initials_avatar([128, 128, 128], 32);
    assert_eq!(&result[0..4], b"RIFF");
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// ── Mandelbrot avatar ───────────────────────────────────────────

// @scenario: contact_card_management :: Generate Mandelbrot avatar
#[test]
fn mandelbrot_avatar_is_valid_webp_within_size_limit() {
    let result = generate_mandelbrot_avatar(0, 256);
    assert_eq!(&result[0..4], b"RIFF", "output must start with RIFF");
    assert_eq!(&result[8..12], b"WEBP", "output must be WebP");
    assert!(
        result.len() <= MAX_AVATAR_SIZE,
        "mandelbrot avatar {} bytes exceeds {} limit",
        result.len(),
        MAX_AVATAR_SIZE,
    );
}

// @scenario: contact_card_management :: Different seeds produce different images
#[test]
fn mandelbrot_different_seeds_produce_different_images() {
    let a = generate_mandelbrot_avatar(0, 128);
    let b = generate_mandelbrot_avatar(5, 128);
    assert_ne!(a, b, "different seeds should produce different images");
}

// @scenario: contact_card_management :: Mandelbrot seed 0 does not panic
#[test]
fn mandelbrot_seed_zero_does_not_panic() {
    let result = generate_mandelbrot_avatar(0, 64);
    assert!(!result.is_empty());
}

// @scenario: contact_card_management :: Mandelbrot large seed
#[test]
fn mandelbrot_large_seed_does_not_panic() {
    let result = generate_mandelbrot_avatar(u64::MAX, 64);
    assert_eq!(&result[0..4], b"RIFF");
}
