// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Camera permission/hardware gate state for the multi-stage exchange
//! engine. Split out of `multi_stage_exchange.rs` (tidy, M3 S5-10) to
//! make room for locale plumbing under the 1000-line file-size limit
//! — this type is fully self-contained and has no dependency on the
//! rest of that file.

/// Camera reason flags in priority order — permission denied wins
/// over hardware unavailable (per investigation §3.1: a denied
/// permission is recoverable while missing hardware is not, but the
/// user should see the actionable affordance first).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CameraGate {
    #[default]
    Available,
    /// OS permission was denied; the frontend can re-prompt via
    /// `GRANT_CAMERA_PERMISSION_ACTION_ID`.
    PermissionDenied,
    /// Hardware reported absent or unusable. No re-prompt path.
    Unavailable,
}

impl CameraGate {
    /// Returns the gate that should win when a new transport-level
    /// signal arrives. Permission-denied beats already-set unavailable
    /// (more actionable) and vice versa never downgrades from
    /// `Unavailable` to `PermissionDenied` — once hardware is gone the
    /// user cannot grant their way out.
    pub(crate) fn promote(self, incoming: CameraGate) -> CameraGate {
        match (self, incoming) {
            (CameraGate::Unavailable, _) => CameraGate::Unavailable,
            (_, CameraGate::Unavailable) => CameraGate::Unavailable,
            (_, CameraGate::PermissionDenied) => CameraGate::PermissionDenied,
            (current, CameraGate::Available) => current,
        }
    }
}

// INLINE_TEST_REQUIRED: tests exercise pub(crate) CameraGate::promote,
// unreachable from an external tests/it integration binary.
#[cfg(test)]
mod camera_gate_tests {
    use super::*;

    // @internal
    #[test]
    fn promote_unavailable_is_terminal() {
        let g = CameraGate::Unavailable.promote(CameraGate::PermissionDenied);
        assert_eq!(g, CameraGate::Unavailable);
        let g = CameraGate::Unavailable.promote(CameraGate::Available);
        assert_eq!(g, CameraGate::Unavailable);
    }

    // @internal
    #[test]
    fn promote_permission_denied_replaces_available() {
        let g = CameraGate::Available.promote(CameraGate::PermissionDenied);
        assert_eq!(g, CameraGate::PermissionDenied);
    }

    // @internal
    #[test]
    fn promote_unavailable_overrides_permission_denied() {
        let g = CameraGate::PermissionDenied.promote(CameraGate::Unavailable);
        assert_eq!(g, CameraGate::Unavailable);
    }

    // @internal
    #[test]
    fn promote_available_is_no_op_for_existing_gate() {
        let g = CameraGate::PermissionDenied.promote(CameraGate::Available);
        assert_eq!(g, CameraGate::PermissionDenied);
    }
}
