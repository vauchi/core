// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one permission to move a contact to another identity
//! (vauchi/private#796; design 2026-11-03-pinned-guardian-policy, point 4
//! and P18). Every identity-changing path takes a [`RecoveryAuthorization`],
//! and only the authorization check makes one, so no path can move a
//! contact on an old-key signature, a confidence score or a sync item of
//! its own.

/// Permission to move the contact holding `old_pk` to `new_pk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryAuthorization {
    old_pk: [u8; 32],
    new_pk: [u8; 32],
}

impl RecoveryAuthorization {
    /// An authorization assumed by a test, to exercise what follows one
    /// without the decision that grants it. Never in a release build: the
    /// `testing` feature is enabled by test and coverage jobs only, and no
    /// other constructor exists until the decision certificate does (P18).
    #[cfg(any(test, feature = "testing"))]
    pub fn assumed_for_tests(old_pk: [u8; 32], new_pk: [u8; 32]) -> Self {
        Self { old_pk, new_pk }
    }

    /// The key of the contact that moves.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// The identity it moves to.
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }
}
