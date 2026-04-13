// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::contact_card::{MAX_AVATAR_SIZE, normalize_avatar};

proptest! {
    // Random bytes should never panic — either Ok(webp) or Err(InvalidFormat)
    #[test]
    fn normalize_never_panics(data in proptest::collection::vec(any::<u8>(), 0..100_000)) {
        // allow(zero_assertions) — intentional: verifies no-panic with random input
        let _ = normalize_avatar(&data);
    }

    // Any valid PNG input always produces valid WebP <= MAX_AVATAR_SIZE
    #[test]
    fn normalize_png_always_valid_webp(
        width in 1u32..500,
        height in 1u32..500,
        r in 0u8..=255,
        g in 0u8..=255,
        b in 0u8..=255,
    ) {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([r, g, b]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let png = buf.into_inner();

        let result = normalize_avatar(&png).unwrap();
        prop_assert_eq!(&result[0..4], b"RIFF");
        prop_assert_eq!(&result[8..12], b"WEBP");
        prop_assert!(result.len() <= MAX_AVATAR_SIZE,
            "Output {} bytes exceeds limit {}", result.len(), MAX_AVATAR_SIZE);
    }
}
