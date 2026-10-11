// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Test-only entry points (`test` and the `testing` feature).

use super::AppEngine;
use crate::ui::ActionResult;
use crate::ui::exchange::ExchangeEngine;
use vauchi_core::exchange::mode::ExchangeMode;

impl AppEngine {
    /// Starts `mode` on the exchange picker as if it had been selected, past
    /// the picker's offered-modes guard, and routes the result like any
    /// action. Keeps the radio-only BLE path of unoffered modes under test
    /// (vauchi/private#271, #506).
    pub fn select_exchange_mode_for_testing(&mut self, mode: ExchangeMode) -> ActionResult {
        let result = self
            .engine
            .as_any_mut()
            .and_then(|engine| engine.downcast_mut::<ExchangeEngine>())
            .expect("the exchange picker must be the active engine")
            .select_mode_for_testing(mode);
        self.finish_engine_result(result)
    }
}
