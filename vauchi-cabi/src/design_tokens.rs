// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI access to the design tokens.
//!
//! ADR-038 makes `themes/tokens.json` the single source of truth for
//! spacing, typography, radii, motion and the touch-target floor, and
//! `DesignTokens::default()` is checked against it. iOS, macOS and Android
//! receive those tokens over UniFFI. Nothing carried them across the C ABI,
//! so linux-qt and windows could not read them however well behaved they
//! were, and both hardcode their own paddings and font sizes as a result.
//!
//! One call returning the whole document, rather than a getter per field:
//! the token set grows, and an ABI that grows with it would have to be
//! re-cut in three repos every time.

use std::os::raw::c_char;

use super::to_c_string;

/// Return the design tokens as JSON.
///
/// The shape matches `vauchi_app::theme::DesignTokens`, which is the same
/// document `themes/tokens.json` holds and the same one iOS, macOS and
/// Android receive over UniFFI.
///
/// Returns null only if the tokens cannot be serialized, which would mean
/// a broken build rather than a runtime condition.
///
/// The caller must free the returned string with `vauchi_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn vauchi_design_tokens_json() -> *mut c_char {
    match std::panic::catch_unwind(|| {
        serde_json::to_string(&vauchi_app::theme::DesignTokens::default())
            .map_or(std::ptr::null_mut(), |json| to_c_string(&json))
    }) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

// INLINE_TEST_REQUIRED: this cdylib/staticlib crate has no Rust integration-test target
#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::*;

    /// The point of the export is that a C shell can act on it, so the
    /// assertion is that the document round-trips back into the same tokens
    /// core holds — not merely that some JSON came out.
    // @internal
    #[test]
    fn the_c_abi_hands_out_the_tokens_core_holds() {
        let ptr = vauchi_design_tokens_json();
        assert!(!ptr.is_null());

        // SAFETY: The C ABI returned a valid string owned by this test.
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        let decoded: vauchi_app::theme::DesignTokens =
            serde_json::from_str(json).expect("tokens JSON decodes back into DesignTokens");

        assert_eq!(decoded, vauchi_app::theme::DesignTokens::default());

        // SAFETY: The pointer is owned by the C ABI and freed exactly once.
        unsafe { crate::vauchi_string_free(ptr) };
    }

    /// The categories a desktop shell has to hardcode today, named
    /// explicitly: a token group silently dropped from the payload would
    /// otherwise still round-trip through serde defaults.
    // @internal
    #[test]
    fn the_payload_carries_the_categories_desktop_shells_hardcode() {
        let ptr = vauchi_design_tokens_json();
        // SAFETY: The C ABI returned a valid string owned by this test.
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        // SAFETY: The pointer is owned by the C ABI and freed exactly once.
        unsafe { crate::vauchi_string_free(ptr) };

        for category in [
            "spacing",
            "spacing_direction",
            "typography",
            "border_radius",
            "touch_target",
            "motion",
            "focus",
        ] {
            assert!(
                json.contains(&format!("\"{category}\"")),
                "the tokens payload has no `{category}`, so a C shell still has to invent it"
            );
        }
    }
}
