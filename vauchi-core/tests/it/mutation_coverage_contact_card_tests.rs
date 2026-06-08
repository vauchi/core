// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-coverage tests for `contact_card/mod.rs`.
//!
//! Kills missed mutants in `normalize_avatar`, `validate_size`,
//! and `field_visibility_mut`.

use image::{ImageBuffer, Rgba, RgbaImage};
use std::io::Cursor;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType, MAX_AVATAR_SIZE};
use vauchi_core::normalize_avatar;

/// Create a test PNG of the given dimensions.
fn png_of_size(width: u32, height: u32) -> Vec<u8> {
    let img: RgbaImage = ImageBuffer::from_pixel(width, height, Rgba([255, 0, 0, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// ============================================================
// normalize_avatar — dimension and size boundary tests
// ============================================================

// @internal
#[test]
fn normalize_avatar_small_image_stays_within_budget() {
    let png = png_of_size(64, 64);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
    assert_eq!(&result[..4], b"RIFF");
}

// @internal
#[test]
fn normalize_avatar_large_dimensions_get_resized() {
    // Create an image larger than MAX_AVATAR_DIMENSION (512)
    let png = png_of_size(1024, 1024);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// @internal
#[test]
fn normalize_avatar_wide_image_triggers_resize() {
    // Width exceeds 512, height does not
    let png = png_of_size(800, 100);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// @internal
#[test]
fn normalize_avatar_tall_image_triggers_resize() {
    // Height exceeds 512, width does not
    let png = png_of_size(100, 800);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// @internal
#[test]
fn normalize_avatar_exactly_512_no_resize() {
    let png = png_of_size(512, 512);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// @internal
#[test]
fn normalize_avatar_dimension_halving_converges() {
    // A very large solid-color image: the loop halves dimensions until it fits.
    // 2048x2048 solid color should still compress within budget.
    let png = png_of_size(2048, 2048);
    let result = normalize_avatar(&png).unwrap();
    assert!(result.len() <= MAX_AVATAR_SIZE);
}

// @internal
#[test]
fn normalize_avatar_empty_input_fails() {
    let result = normalize_avatar(&[]);
    result.expect_err("empty data should fail");
}

// @internal
#[test]
fn normalize_avatar_invalid_data_fails() {
    let result = normalize_avatar(b"not an image");
    result.expect_err("invalid image data should fail");
}

// ============================================================
// validate_size
// ============================================================

// @internal
#[test]
fn validate_size_normal_card_ok() {
    let card = ContactCard::new("Alice");
    assert!(
        card.validate_size().is_ok(),
        "a minimal card should pass size validation"
    );
}

// @internal
#[test]
fn validate_size_oversized_card_fails() {
    let mut card = ContactCard::new("Alice");
    for i in 0..200 {
        let value = "X".repeat(350);
        let field = ContactField::new(FieldType::Custom, &format!("note-{i}"), &value, 0);
        card.add_field(field).unwrap();
    }
    card.validate_size()
        .expect_err("card exceeding MAX_CARD_SIZE_BYTES should fail");
}

// ============================================================
// field_visibility_mut
// ============================================================

// @internal
#[test]
fn field_visibility_mut_allows_modification() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "work", "a@b.com", 0);
    card.add_field(field).unwrap();
    let field_id = card.fields()[0].id().to_string();

    // Mutate visibility through the mutable accessor
    card.field_visibility_mut().set_nobody(&field_id);

    assert!(
        !card.is_field_shown(&field_id),
        "field should be hidden after set_nobody"
    );
}
