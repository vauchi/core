// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common Test Utilities
//!
//! Shared helpers, fixtures, and utilities used across test modules.
//! This module provides reusable test infrastructure to reduce duplication.

#[allow(dead_code)]
pub mod fixtures;
#[allow(dead_code)]
pub mod helpers;
#[allow(dead_code)]
pub mod strategies;
#[allow(dead_code)]
pub mod verifiers;

#[allow(dead_code)]
pub mod app_engine_helpers;
#[allow(dead_code)]
pub mod field_validation_helpers;
#[cfg(feature = "network-http")]
#[allow(dead_code)]
pub mod mock_relay;
