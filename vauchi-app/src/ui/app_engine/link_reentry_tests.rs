// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{AppEngine, AppScreen};
use crate::ui::exchange::success::ExchangeSuccessSummary;
use crate::ui::{EngineUpdate, LinkExchangeUpdate, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::exchange::mode::ExchangeMode;

// @internal
#[test]
fn new_link_attempt_after_a_terminal_flow_presents_share_again() {
    for terminal in [
        LinkExchangeUpdate::Succeeded(ExchangeSuccessSummary {
            peer_name: "Bob".into(),
            ..Default::default()
        }),
        LinkExchangeUpdate::Failed("Relay unavailable".into()),
    ] {
        let mut core = Vauchi::in_memory().expect("in-memory storage");
        core.create_identity("Alice").expect("Alice");
        let mut app = AppEngine::new(core);
        app.navigate_to(AppScreen::LinkExchange);
        app.engine
            .apply_update(EngineUpdate::LinkExchange(terminal));
        assert_eq!(
            app.current_screen()
                .contextual_actions
                .iter()
                .find(|action| action.id == "share")
                .map(|action| action.enabled),
            None
        );

        app.navigate_to(AppScreen::Contacts);
        app.navigate_to(AppScreen::Exchange);
        let _ = app.select_exchange_mode_for_testing(ExchangeMode::Link);

        assert_eq!(
            app.current_screen()
                .contextual_actions
                .iter()
                .find(|action| action.id == "share")
                .map(|action| action.enabled),
            Some(true),
            "a new attempt must offer Share after the earlier flow ended"
        );
    }
}
