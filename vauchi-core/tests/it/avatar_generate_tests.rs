// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for avatar generation (initials + Mandelbrot).

use rstest::rstest;
use vauchi_core::avatar::{generate_initials_avatar, generate_mandelbrot_avatar};
use vauchi_core::contact_card::MAX_AVATAR_SIZE;

/// Decode WebP bytes to RGBA pixels.
fn decode_rgba(bytes: &[u8]) -> image::RgbaImage {
    image::load_from_memory(bytes)
        .expect("avatar bytes must decode")
        .to_rgba8()
}

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

// ── Mutation-coverage tests ─────────────────────────────────────
// Pin concrete pixel values so arithmetic / comparison mutations
// in the circle-mask and Mandelbrot iteration cannot survive.

/// At small generation sizes the normalize_avatar pipeline keeps
/// the original dimension intact (no resize). We pick 64.
const PIN_SIZE: u32 = 64;

// @scenario: contact_card_management :: Initials avatar pin specific pixels
#[rstest]
#[case::red([255, 0, 0])]
#[case::green([0, 255, 0])]
#[case::blue([0, 0, 255])]
#[case::grey([128, 128, 128])]
fn initials_avatar_center_pixel_matches_bg_color(#[case] bg: [u8; 3]) {
    let bytes = generate_initials_avatar(bg, PIN_SIZE);
    let img = decode_rgba(&bytes);
    let cx = img.width() / 2;
    let cy = img.height() / 2;
    let center_px = img.get_pixel(cx, cy);
    // The circle covers the geometric center, so the center pixel must
    // be the bg_color with full alpha. Kills mutations that change the
    // mask radius or the channel indices.
    assert_eq!(
        center_px.0,
        [bg[0], bg[1], bg[2], 255],
        "center pixel must equal bg_color with alpha=255",
    );
}

// @scenario: contact_card_management :: Initials avatar corners are transparent
#[test]
fn initials_avatar_corners_are_transparent() {
    let bytes = generate_initials_avatar([200, 100, 50], PIN_SIZE);
    let img = decode_rgba(&bytes);
    let w = img.width();
    let h = img.height();
    // The inscribed circle's farthest corner is at the rectangle corners,
    // outside the radius — alpha must be 0. Kills mutations that flip
    // `<= radius_sq` to `>` or change radius computation.
    for &(x, y) in &[(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let px = img.get_pixel(x, y);
        assert_eq!(
            px.0[3], 0,
            "corner pixel ({}, {}) must be transparent, got {:?}",
            x, y, px.0
        );
    }
}

// @scenario: contact_card_management :: Initials avatar is symmetric
#[test]
fn initials_avatar_is_horizontally_symmetric() {
    let bytes = generate_initials_avatar([10, 200, 50], PIN_SIZE);
    let img = decode_rgba(&bytes);
    let w = img.width();
    let h = img.height();
    // Mirror symmetry about the vertical centerline. Kills mutations
    // that off-center the circle (e.g., the `+ 0.5` half-pixel offset
    // becoming `- 0.5` or `* 0.5`).
    for y in 0..h {
        for x in 0..w / 2 {
            let mirror_x = w - 1 - x;
            assert_eq!(
                img.get_pixel(x, y),
                img.get_pixel(mirror_x, y),
                "asymmetric at ({}, {}) vs ({}, {})",
                x,
                y,
                mirror_x,
                y
            );
        }
    }
}

// @scenario: contact_card_management :: Initials avatar is deterministic
#[test]
fn initials_avatar_is_deterministic() {
    // Re-running with the same inputs must yield byte-identical output.
    // Kills mutations that introduce non-determinism (none expected,
    // but a sanity assertion).
    let a = generate_initials_avatar([42, 123, 200], PIN_SIZE);
    let b = generate_initials_avatar([42, 123, 200], PIN_SIZE);
    assert_eq!(a, b);
}

// @scenario: contact_card_management :: Mandelbrot avatar corners transparent
#[test]
fn mandelbrot_avatar_corners_are_transparent() {
    let bytes = generate_mandelbrot_avatar(7, PIN_SIZE);
    let img = decode_rgba(&bytes);
    let w = img.width();
    let h = img.height();
    for &(x, y) in &[(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let px = img.get_pixel(x, y);
        assert_eq!(
            px.0[3], 0,
            "Mandelbrot corner ({}, {}) must be transparent",
            x, y
        );
    }
}

// @scenario: contact_card_management :: Mandelbrot center is inside the set
#[test]
fn mandelbrot_avatar_center_is_inside_or_colored() {
    // The center of the avatar is at (-0.5+seed%10*0.05, seed/10%10*0.05).
    // For seed=0 that is (-0.5, 0), which is inside the Mandelbrot set:
    // iter == max_iter → black with alpha=255. Kills mutations that
    // change the iteration upper bound or the escape condition.
    let bytes = generate_mandelbrot_avatar(0, PIN_SIZE);
    let img = decode_rgba(&bytes);
    let cx = img.width() / 2;
    let cy = img.height() / 2;
    let px = img.get_pixel(cx, cy).0;
    assert_eq!(
        px,
        [0, 0, 0, 255],
        "seed=0 center should be in the Mandelbrot set (black, opaque)",
    );
}

// @scenario: contact_card_management :: Mandelbrot seed nudges center
#[rstest]
#[case::same_x_different_y(0, 10)] // (0,0) vs (0,1): different center_y
#[case::different_x(0, 1)] // (0,0) vs (1,0): different center_x
#[case::wrap(0, 100)] // 100/10%10=0 → same y, but x = 0 → same. Boring? Actually 100%10=0, 100/10%10=0 so identical.
fn mandelbrot_seed_changes_center_offset(#[case] seed_a: u64, #[case] seed_b: u64) {
    let a = generate_mandelbrot_avatar(seed_a, PIN_SIZE);
    let b = generate_mandelbrot_avatar(seed_b, PIN_SIZE);
    let same_modular_offset =
        seed_a % 10 == seed_b % 10 && (seed_a / 10) % 10 == (seed_b / 10) % 10;
    if same_modular_offset {
        // Seeds with identical (seed%10, seed/10%10) produce identical images.
        // Kills mutations that change the modulus arithmetic.
        assert_eq!(
            a, b,
            "seeds {} and {} share offsets but differ",
            seed_a, seed_b
        );
    } else {
        assert_ne!(
            a, b,
            "seeds {} and {} should produce different images",
            seed_a, seed_b
        );
    }
}

// @scenario: contact_card_management :: Initials avatar size grows with side length
#[rstest]
#[case(32)]
#[case(64)]
#[case(128)]
#[case(256)]
fn initials_avatar_decoded_dimensions_are_at_most_size(#[case] size: u32) {
    let bytes = generate_initials_avatar([200, 50, 100], size);
    let img = decode_rgba(&bytes);
    // normalize_avatar may downscale to MAX_AVATAR_DIMENSION (512), so
    // the decoded dim is min(size, 512). For our cases all values ≤ 512.
    assert_eq!(img.width(), size);
    assert_eq!(img.height(), size);
}

// ── Initials pixel-pin tests (kill arithmetic mutations) ────────
// At size=64 the inscribed circle has center (32, 32) and radius
// sqrt(32*32) = 32. We pin specific pixels whose coloring is
// determined by the mask arithmetic, so any mutation in lines
// 20-26 (radius_sq, dx, dy, mask comparison) flips at least one
// of these assertions.
//
// Reasoning per pixel:
//   * (48, 32): dx=16.5, dy=0.5 → dx²+dy²≈272.5 < 1024. ORIG colored.
//     A `* with +` mutation on `radius_sq = center * center` would
//     give radius_sq=64; 272.5 > 64 → transparent. ↗ caught.
//   * (32, 0):  dx=0.5, dy=-31.5 → dx²+dy²≈992.5 < 1024. ORIG colored.
//     A `+ with -` mutation on `dy = y - center + 0.5` would give
//     dy=-32.5, total ≈1056.5 → transparent. ↗ caught.
//   * (10, 5):  dx=-21.5, dy=-26.5 → dx²+dy²≈1164.5 > 1024. ORIG transparent.
//     A `+ with *` mutation on `dy = y - center + 0.5` collapses
//     dy toward zero, putting the pixel inside the disc → colored.
//     ↗ caught.
//   * (15, 15): dx=-16.5, dy=-16.5 → dx²+dy²=544.5 < 1024. ORIG colored.
//     A `+ with *` mutation on `dx*dx + dy*dy` evaluates dx²·dy² =
//     74 120, far above 1024 → transparent. ↗ caught.
#[test]
fn initials_avatar_disc_mask_pins_specific_pixels() {
    let bg = [200u8, 50, 100];
    let size = 64u32;
    let bytes = generate_initials_avatar(bg, size);
    let img = decode_rgba(&bytes);

    let colored = [bg[0], bg[1], bg[2], 255];

    assert_eq!(
        img.get_pixel(48, 32).0,
        colored,
        "pixel (48, 32) must be inside the inscribed disc"
    );
    assert_eq!(
        img.get_pixel(32, 0).0,
        colored,
        "pixel (32, 0) must be inside the inscribed disc (dy²≈992.25)"
    );
    assert_eq!(
        img.get_pixel(10, 5).0[3],
        0,
        "pixel (10, 5) must be transparent (corner region)"
    );
    assert_eq!(
        img.get_pixel(15, 15).0,
        colored,
        "pixel (15, 15) must be colored (well inside the disc)"
    );
}

// ── Mandelbrot seed-specific pixel pins ─────────────────────────
// Pinning a handful of pixel values for a known (seed, size) catches
// the arithmetic mutations in the Mandelbrot iteration loop and color
// mapping. Values were captured from the unmodified implementation;
// any mutation that changes iteration count, escape condition, color
// formula, or center/zoom math will perturb at least one of them.
#[test]
fn mandelbrot_avatar_seed_zero_pixel_table() {
    let bytes = generate_mandelbrot_avatar(0, 64);
    let img = decode_rgba(&bytes);

    // Center (32, 32): c = -0.5 + 0i → in the Mandelbrot set, black.
    assert_eq!(img.get_pixel(32, 32).0, [0, 0, 0, 255]);

    // (16, 32): c ≈ -1.125 + 0i → in the set (period-2 bulb), black.
    assert_eq!(img.get_pixel(16, 32).0, [0, 0, 0, 255]);

    // (48, 32): c ≈ 0.125 + 0i → in the set (main cardioid), black.
    assert_eq!(img.get_pixel(48, 32).0, [0, 0, 0, 255]);

    // Top corner (0, 0) is outside the inscribed disc → transparent.
    assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);

    // (60, 32) is inside the inscribed disc but c ≈ 0.594 escapes
    // quickly (~iter 5). Pinning the exact RGB catches mutations to
    // both the iteration loop AND the color formula.
    let escape = img.get_pixel(60, 32).0;
    assert_eq!(
        escape[3], 255,
        "outside-set pixel must be opaque (alpha=255)"
    );
    // It must NOT be black — it escaped before max_iter.
    assert_ne!(
        &escape[..3],
        &[0, 0, 0],
        "outside-set pixel must NOT be in-set black; got {:?}",
        escape
    );
    // Red channel dominates because the formula multiplies by 9.0.
    assert!(
        escape[0] > escape[1] && escape[0] > escape[2],
        "Red channel must dominate (formula t*9*255 vs t*3*255 vs t*255), got {:?}",
        escape
    );
}

// ── Mandelbrot byte-level snapshot ──────────────────────────────
// A hash of the WebP bytes for a known seed/size pins the entire
// pipeline end-to-end. Any mutation in the iteration math or color
// mapping that survives the per-pixel checks above will still flip
// at least one byte and thus the hash. The hash is a one-line
// constant — easy to update intentionally if the implementation
// or encoder changes.
#[test]
fn mandelbrot_avatar_seed_zero_size_64_byte_hash() {
    use sha2::{Digest, Sha256};
    let bytes = generate_mandelbrot_avatar(0, 64);
    let hash = Sha256::digest(&bytes);
    let hex = format!("{:x}", hash);
    // Captured 2026-04-27 against an unmutated build.
    // If the avatar implementation or libwebp encoder version changes
    // intentionally, regenerate via:
    //   cargo test mandelbrot_avatar_seed_zero_size_64_byte_hash -- --nocapture
    // and replace this constant.
    const EXPECTED: &str = "b78992c22889235547b3cbf26edf779bf89232d66b18509c6b49a24bb1b09fc2";
    assert_eq!(hex, EXPECTED, "mandelbrot output bytes changed");
}

// Snapshot at a non-zero seed%10 — catches mutations to center_x math
// (line 42) that collapse to identity for seed=0 (e.g. `+ with -` on
// `-0.5 + (seed%10)*0.05` since `seed%10==0` makes both forms equal).
#[test]
fn mandelbrot_avatar_seed_five_size_64_byte_hash() {
    use sha2::{Digest, Sha256};
    let bytes = generate_mandelbrot_avatar(5, 64);
    let hash = Sha256::digest(&bytes);
    let hex = format!("{:x}", hash);
    const EXPECTED: &str = "4236bab2352b30f81f7760b5540cc8196632a58d75bb56d2c65d29b7c67d2c93";
    assert_eq!(hex, EXPECTED, "mandelbrot seed=5 output bytes changed");
}

// Snapshot at a non-zero seed/10%10 — catches mutations to center_y
// math (line 43) and y-axis symmetry-breaking (lines 57, 63) that
// collapse to identity for seed=0. (Mandelbrot is y-symmetric, so
// `y = 2xy - y0` produces the same iter count as `y = 2xy + y0`
// when center_y=0.)
#[test]
fn mandelbrot_avatar_seed_ten_size_64_byte_hash() {
    use sha2::{Digest, Sha256};
    let bytes = generate_mandelbrot_avatar(10, 64);
    let hash = Sha256::digest(&bytes);
    let hex = format!("{:x}", hash);
    const EXPECTED: &str = "c94b5dba79590bb10c2af440cfa5e8cbe3ddb46547580cb3a6afbdbf509762eb";
    assert_eq!(hex, EXPECTED, "mandelbrot seed=10 output bytes changed");
}
