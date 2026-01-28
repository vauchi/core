// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration Tests for Vauchi Core
//!
//! These tests verify complete workflows from identity creation through contact exchange
//! and synchronization.
//!
//! Run with: cargo test --test integration

mod contact_workflow_test;
mod identity_workflow_test;
mod sync_workflow_test;
