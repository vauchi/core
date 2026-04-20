// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Test-support primitives for the Layer 1 reachability harness.
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]` so
//! downstream crates enable `test-support` in their `dev-dependencies`
//! to drive reachability properties against `WorkflowEngine` impls
//! in their own integration tests.
//!
//! # Entry-screen convention (Phase 0 Task 0.2 audit)
//!
//! The plan proposed an `initial_screen(&self) -> ScreenModel`
//! method per engine. Audit found every engine already exposes a
//! deterministic entry via its `::new(...)` constructor paired with
//! the trait-required `WorkflowEngine::current_screen(&self)`
//! (`ui/engine.rs:17`). Harnesses therefore use
//! `Engine::new(...).current_screen()` as the canonical starting
//! point; no per-engine method is added.
//!
//! Plan:
//! `_private/docs/planning/todo/2026-04-20-frontend-correctness-strategy-plan.md`.

pub mod screen_walker;

pub use screen_walker::{all_reachable_screens, walk_actions};
