// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **join** (responder) lifecycle on `AppEngine` — M5 B3 Slice 3.
//!
//! Mirrors `device_link_initiator.rs` for the fresh device that receives a
//! `DeviceLinkJoinInvitation`. Core owns the poll-driven
//! `DeviceLinkResponderMachine`, builds it on entry to
//! `AppScreen::DeviceLinkJoin`, and tears it down on exit. The humble
//! `DeviceLinkJoinEngine` only renders state pushed via
//! `DeviceLinkJoinUpdate`.
//!
//! On `ResponseReady`, core calls `Vauchi::adopt_device_link_response`,
//! persists the joined identity, and navigates home. Frontends never see
//! the master seed or decide when to navigate.

use super::{AppEngine, AppScreen};
use crate::orchestrator::device_link_relay::DeviceLinkBroker;
use crate::orchestrator::device_link_responder_machine::{
    DeviceLinkResponderMachine, ResponderEvent,
};
use crate::ui::{
    ActionResult, DeviceLinkJoinEngine, DeviceLinkJoinUpdate, EngineUpdate, ScreenModel,
};

use std::net::SocketAddr;
use std::time::Duration;
use vauchi_core::exchange::DeviceLinkJoinInvitation;

use vauchi_core::network::HttpTransport;

use crate::orchestrator::local_client::RemoteRendezvousBroker;

/// QR-expiry / relay-poll budget (ADR-035 device-link window = 300 s).
pub(crate) const QR_WINDOW_SECS: u64 = 300;

/// Holds the responder machine plus the broker it posts on. The broker is
/// boxed so tests can inject a fake without changing the holder's shape.
pub(crate) struct DeviceLinkResponderHolder {
    machine: DeviceLinkResponderMachine,
    broker: Box<dyn DeviceLinkBroker + Send>,
}

impl AppEngine {
    /// Build / drop the responder machine on entry / exit of the
    /// `DeviceLinkJoin` screen. Called from `navigate_to_internal`.
    pub(super) fn sync_device_link_responder_lifecycle(
        &mut self,
        old: &AppScreen,
        new: &AppScreen,
    ) {
        let was = matches!(old, AppScreen::DeviceLinkJoin { .. });
        let is = matches!(new, AppScreen::DeviceLinkJoin { .. });
        match (was, is) {
            (true, false) => self.cancel_device_link_responder(),
            (false, true) => {
                // Build the machine lazily on the first poll tick after the
                // user confirms the device name. Until then the engine sits
                // on the name-entry / posting-request screen with no relay I/O.
            }
            _ => {}
        }
    }

    /// Parse `invitation_url` and start the responder machine with the
    /// given device name. Called from `route_result` when the join engine
    /// emits `ActionResult::DeviceLinkJoinStart`. No relay I/O here — the
    /// first `advance_device_link_responder_session` posts the request.
    pub(super) fn start_device_link_responder(
        &mut self,
        invitation_url: &str,
        device_name: String,
    ) -> Result<(), String> {
        if self.device_link_responder.is_some() {
            return Ok(());
        }
        let invitation = DeviceLinkJoinInvitation::parse_url(invitation_url)
            .map_err(|e| format!("invalid invitation: {e}"))?;
        let broker = self.build_device_link_broker(&invitation)?;
        let machine = DeviceLinkResponderMachine::new(invitation, device_name, QR_WINDOW_SECS)
            .map_err(|e| format!("cannot start responder: {e}"))?;
        self.device_link_responder = Some(DeviceLinkResponderHolder { machine, broker });
        Ok(())
    }

    /// Cancel + drop the active responder machine. Idempotent.
    pub fn cancel_device_link_responder(&mut self) {
        if let Some(mut holder) = self.device_link_responder.take() {
            let _ = holder.machine.cancel();
        }
    }

    /// One non-blocking relay step. Called from `poll_notifications`.
    /// Returns true if the join screen changed.
    pub(crate) fn advance_device_link_responder_session(&mut self) -> bool {
        let now = self.vauchi.clock().unix_seconds();
        let event = match self.device_link_responder.as_mut() {
            Some(holder) => holder.machine.advance(holder.broker.as_ref(), now),
            None => return false,
        };
        self.apply_responder_event(event)
    }

    /// Translate a responder event into a `DeviceLinkJoinUpdate` for the
    /// active engine. Handles terminal adoption on `ResponseReady`.
    fn apply_responder_event(&mut self, event: ResponderEvent) -> bool {
        match event {
            ResponderEvent::None => false,
            ResponderEvent::RequestPosted { confirmation_code } => self
                .device_link_join_request_posted(confirmation_code)
                .is_some(),
            ResponderEvent::ResponseReady => self.on_device_link_response_ready(),
            ResponderEvent::Failed { reason } => self.device_link_join_failed(reason).is_some(),
        }
    }

    /// The request is posted; show the confirmation code.
    pub fn device_link_join_request_posted(
        &mut self,
        confirmation_code: String,
    ) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLinkJoin(
                DeviceLinkJoinUpdate::RequestPosted { confirmation_code },
            ))
            .then(|| self.engine.current_screen())
    }

    /// The encrypted response arrived. Move the engine to Completing,
    /// adopt the identity, then Complete and navigate home. If adoption
    /// fails, move to Failed.
    fn on_device_link_response_ready(&mut self) -> bool {
        let response = match self
            .device_link_responder
            .as_mut()
            .and_then(|holder| holder.machine.take_response())
        {
            Some(response) => response,
            None => {
                return self
                    .device_link_join_failed("response_missing".to_string())
                    .is_some();
            }
        };

        let device_name = self
            .engine
            .as_any()
            .and_then(|any| any.downcast_ref::<DeviceLinkJoinEngine>())
            .map(|engine| engine.device_name().to_string())
            .unwrap_or_else(|| "New Device".to_string());

        let _ = self.engine.apply_update(EngineUpdate::DeviceLinkJoin(
            DeviceLinkJoinUpdate::ResponseReady,
        ));

        match self
            .vauchi
            .adopt_device_link_response(&response, device_name)
        {
            Ok(()) => {
                self.device_link_responder = None;
                let _ = self.engine.apply_update(EngineUpdate::DeviceLinkJoin(
                    DeviceLinkJoinUpdate::Completed,
                ));
                // Navigate to the post-join landing screen. After adoption an
                // identity exists, so MyInfo is appropriate (Lock will intercept
                // if a password is set on the next foreground event).
                let _ = self.navigate_to_internal(AppScreen::MyInfo);
                true
            }
            Err(e) => {
                self.device_link_responder = None;
                self.device_link_join_failed(format!("adopt_failed: {e}"))
                    .is_some()
            }
        }
    }

    /// Terminal failure on the join screen.
    pub fn device_link_join_failed(&mut self, reason: String) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLinkJoin(DeviceLinkJoinUpdate::Failed(
                reason,
            )))
            .then(|| self.engine.current_screen())
    }

    /// Choose the broker this invitation points at.
    ///
    /// A local rendezvous wins when present: the initiator is hosting the
    /// ceremony itself, so there is no relay in it at all (ADR-070). The
    /// address was already bounded at parse time — it must be a socket
    /// address on a link-local range — because it arrives in a scanned QR.
    ///
    /// Core makes this choice, never the shell (ADR-021/066): the shell
    /// reports what it scanned and renders what it is sent.
    fn build_device_link_broker(
        &self,
        invitation: &DeviceLinkJoinInvitation,
    ) -> Result<Box<dyn DeviceLinkBroker + Send>, String> {
        if let Some(addr) = invitation.local_rendezvous.as_deref() {
            let parsed: SocketAddr = addr
                .parse()
                .map_err(|_| "local rendezvous is not a socket address".to_string())?;
            let timeout =
                Duration::from_millis(self.vauchi.config().relay.connect_timeout_ms.max(10_000));
            return Ok(Box::new(RemoteRendezvousBroker::new(
                parsed, timeout, timeout,
            )));
        }
        Ok(Box::new(self.build_device_link_transport(
            invitation.relay_url.as_deref(),
        )?))
    }

    /// Build a fail-closed transport for the device-link responder.
    ///
    /// A foreign invitation relay cannot be used until the invitation also
    /// carries its distinct outer OHTTP endpoint, gateway key, and pin set.
    /// Untouched by ADR-070: a peer hosting its own rendezvous is handled
    /// above and never reaches here, so this refusal keeps its original
    /// meaning rather than learning to parse strings.
    fn build_device_link_transport(
        &self,
        relay_url: Option<&str>,
    ) -> Result<HttpTransport, String> {
        if relay_url.is_some() {
            return Err("foreign relay invitation lacks outer OHTTP metadata".to_string());
        }
        let connect_timeout_ms = self.vauchi.config().relay.connect_timeout_ms;
        Ok(self.vauchi.build_relay_transport(
            &self.vauchi.config().relay.server_url,
            connect_timeout_ms.max(10_000),
        ))
    }

    /// Route `ActionResult::DeviceLinkJoinStart` into the responder machine.
    /// Lives here so the whole handler is gated behind the same feature cfg
    /// as the responder machine itself.
    pub(super) fn route_device_link_join_start(&mut self, device_name: String) -> ActionResult {
        if let AppScreen::DeviceLinkJoin { invitation_url } = &self.screen {
            let url = invitation_url.clone();
            if let Err(e) = self.start_device_link_responder(&url, device_name) {
                return ActionResult::ShowAlert {
                    title: self.t("device_link.invalid_invitation_title"),
                    message: e,
                };
            }
            let _ = self.engine.apply_update(EngineUpdate::DeviceLinkJoin(
                DeviceLinkJoinUpdate::NameAccepted,
            ));
        }
        ActionResult::UpdateScreen(self.engine.current_screen())
    }
}

// INLINE_TEST_REQUIRED: tests drive private AppEngine responder integration and state transitions.
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::api::Vauchi;
    use vauchi_core::exchange::{DeviceLinkInitiator, DeviceLinkJoinInvitation};
    use vauchi_core::identity::{DeviceRegistry, Identity};

    const NOW: u64 = 1_700_000_000;

    fn create_test_registry(identity: &Identity) -> DeviceRegistry {
        let master_seed = [0x42u8; 32];
        DeviceRegistry::new(
            identity.device_info().to_registered(&master_seed),
            identity.signing_keypair(),
        )
    }

    fn sample_invitation_and_initiator(now: u64) -> (String, DeviceLinkInitiator) {
        let identity = Identity::create("Alice", 0);
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new([0x11u8; 32], &identity, registry, now);
        let url = DeviceLinkJoinInvitation {
            qr_data: initiator.qr().to_data_string(),
            broker_code: "BROKER42".to_string(),
            relay_url: None,
            local_rendezvous: None,
        }
        .to_url();
        (url, initiator)
    }

    fn sample_invitation_url(now: u64) -> String {
        sample_invitation_and_initiator(now).0
    }

    fn fresh_app_engine() -> AppEngine {
        let vauchi = Vauchi::in_memory().expect("in-memory vauchi");
        AppEngine::new(vauchi)
    }

    // @internal
    #[test]
    fn open_invitation_navigates_to_join_screen() {
        let mut app = fresh_app_engine();
        assert!(!app.vauchi().has_identity());

        let screen = app
            .open_device_link_invitation(&sample_invitation_url(NOW))
            .expect("invitation accepted");

        assert_eq!(screen.screen_id, "device_link_join");
        assert!(matches!(
            app.current_app_screen(),
            AppScreen::DeviceLinkJoin { .. }
        ));
    }

    // @internal
    #[test]
    fn open_invitation_rejects_when_identity_exists() {
        let mut app = fresh_app_engine();
        let _ = app.vauchi.create_identity("Alice");

        let err = app
            .open_device_link_invitation(&sample_invitation_url(NOW))
            .expect_err("invitation rejected when identity exists");

        assert!(err.contains("already has an identity"));
    }

    // @internal
    #[test]
    fn start_responder_builds_machine_and_is_idempotent() {
        let mut app = fresh_app_engine();
        let url = sample_invitation_url(NOW);
        let _ = app.open_device_link_invitation(&url);

        let device_name = app
            .engine
            .as_any()
            .and_then(|any| any.downcast_ref::<DeviceLinkJoinEngine>())
            .map(|e| e.device_name().to_string())
            .unwrap_or_else(|| "New Device".to_string());

        assert!(app.device_link_responder.is_none());
        app.start_device_link_responder(&url, device_name.clone())
            .expect("responder starts");
        assert!(app.device_link_responder.is_some());

        // Second start is a no-op; the existing machine is preserved.
        app.start_device_link_responder(&url, "Other Name".to_string())
            .expect("second start is idempotent");
        assert!(app.device_link_responder.is_some());
    }

    // @scenario: device_sync:Foreign relay invitation fails closed
    #[test]
    fn start_responder_rejects_relay_without_outer_ohttp_metadata() {
        let mut app = fresh_app_engine();
        let identity = Identity::create("Alice", 0);
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new([0x11u8; 32], &identity, registry, NOW);
        let url = DeviceLinkJoinInvitation {
            qr_data: initiator.qr().to_data_string(),
            broker_code: "BROKER42".to_string(),
            relay_url: Some("https://foreign-relay.example".to_string()),
            local_rendezvous: None,
        }
        .to_url();
        let _ = app.open_device_link_invitation(&url);

        let error = app
            .start_device_link_responder(&url, "New Device".to_string())
            .expect_err("foreign relay without an outer OHTTP endpoint must fail closed");

        assert!(
            error.contains("outer OHTTP metadata"),
            "failure should explain the privacy requirement, got: {error}"
        );
        assert!(app.device_link_responder.is_none());
    }

    // @internal
    #[test]
    fn apply_updates_transition_join_engine() {
        let mut app = fresh_app_engine();
        let _ = app.open_device_link_invitation(&sample_invitation_url(NOW));

        assert!(app.engine.apply_update(EngineUpdate::DeviceLinkJoin(
            DeviceLinkJoinUpdate::NameAccepted
        )));
        assert_eq!(
            app.engine.current_screen().screen_id,
            "device_link_join_posting"
        );

        assert!(app.engine.apply_update(EngineUpdate::DeviceLinkJoin(
            DeviceLinkJoinUpdate::RequestPosted {
                confirmation_code: "123456".into(),
            }
        )));
        assert_eq!(
            app.engine.current_screen().screen_id,
            "device_link_join_confirm"
        );
    }

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    use crate::orchestrator::device_link_relay::{ClaimPayload, DeviceLinkBroker};
    use vauchi_core::exchange::ProximityProof;
    use vauchi_core::network::NetworkError;

    type CompleteQueue = Arc<Mutex<VecDeque<Result<Option<String>, NetworkError>>>>;

    /// Scriptable broker fake for AppEngine-level join tests. Uses `Arc<Mutex>`
    /// so the type is `Send` and cloneable, matching the boxed broker bound and
    /// letting the test read deposited claims after the broker is boxed.
    #[derive(Clone)]
    struct FakeBroker {
        claims: Arc<Mutex<Vec<(String, String)>>>,
        complete_queue: CompleteQueue,
    }

    impl FakeBroker {
        fn new() -> Self {
            Self {
                claims: Arc::new(Mutex::new(Vec::new())),
                complete_queue: Arc::new(Mutex::new(VecDeque::new())),
            }
        }
        fn push_complete(&self, r: Result<Option<String>, NetworkError>) {
            self.complete_queue
                .lock()
                .expect("fake broker lock")
                .push_back(r);
        }
        fn claims(&self) -> Vec<(String, String)> {
            self.claims.lock().expect("fake broker lock").clone()
        }
    }

    impl DeviceLinkBroker for FakeBroker {
        fn exchange_offer(
            &self,
            _payload_b64: &str,
            _expires_secs: Option<u64>,
        ) -> Result<String, NetworkError> {
            Ok("BROKER".to_string())
        }
        fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
            self.claims
                .lock()
                .expect("fake broker lock")
                .push((code.to_string(), response_b64.to_string()));
            Ok(String::new())
        }
        fn exchange_complete(&self, _code: &str) -> Result<Option<String>, NetworkError> {
            self.complete_queue
                .lock()
                .expect("fake broker lock")
                .pop_front()
                .unwrap_or(Ok(None))
        }
    }

    // @internal
    #[test]
    fn advance_posts_request_and_updates_engine() {
        let mut app = fresh_app_engine();
        let now = app.vauchi.clock().unix_seconds();
        let url = sample_invitation_url(now);
        let _ = app.open_device_link_invitation(&url);

        // Build the responder machine directly so we can inject the fake broker.
        let device_name = app
            .engine
            .as_any()
            .and_then(|any| any.downcast_ref::<DeviceLinkJoinEngine>())
            .map(|e| e.device_name().to_string())
            .unwrap_or_else(|| "New Device".to_string());
        app.start_device_link_responder(&url, device_name)
            .expect("responder starts");

        // Replace the real transport with the fake broker on the holder.
        let fake = FakeBroker::new();
        app.device_link_responder.as_mut().unwrap().broker = Box::new(fake);

        let changed = app.advance_device_link_responder_session();

        assert!(
            changed,
            "advance must change the screen after posting the request"
        );
        let screen = app.engine.current_screen();
        assert_eq!(
            screen.screen_id, "device_link_join_confirm",
            "expected confirm screen, got {screen:?}"
        );
    }

    // @internal
    #[test]
    fn full_join_adopts_identity_and_navigates_home() {
        use crate::ui::engine::WorkflowEngine;
        use crate::ui::{ActionResult, UserAction};

        let mut app = fresh_app_engine();
        let now = app.vauchi.clock().unix_seconds();
        let (url, initiator) = sample_invitation_and_initiator(now);

        // Fresh device opens the invitation.
        let screen = app
            .open_device_link_invitation(&url)
            .expect("invitation accepted");
        assert_eq!(screen.screen_id, "device_link_join");
        assert!(!app.vauchi().has_identity());

        // User confirms the device name; AppEngine starts the responder.
        let result = app.handle_action(UserAction::ActionPressed {
            action_id: "join".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(
            app.engine.current_screen().screen_id,
            "device_link_join_posting"
        );
        assert!(app.device_link_responder.is_some());

        // Replace the real transport with a fake broker to drive the handshake.
        let fake = FakeBroker::new();
        app.device_link_responder.as_mut().unwrap().broker = Box::new(fake.clone());

        // First advance posts the request and shows the confirmation code.
        assert!(app.advance_device_link_responder_session());
        assert_eq!(
            app.engine.current_screen().screen_id,
            "device_link_join_confirm"
        );

        // Recover the claim the responder deposited and build the initiator's response.
        let claims = fake.claims();
        let (_, claim_b64) = claims.into_iter().next().expect("responder posted a claim");
        let claim: ClaimPayload = {
            let bytes = BASE64.decode(&claim_b64).expect("claim is valid base64");
            serde_json::from_slice(&bytes).expect("claim parses")
        };
        let (_confirmation, request) = initiator
            .prepare_confirmation(&claim.request)
            .expect("prepare confirmation");
        let proof = ProximityProof::Ultrasonic {
            challenge_response: initiator.proximity_challenge(),
            verified_at: now,
        };
        let (encrypted_response, _registry, _new_device) = initiator
            .confirm_link(&request, &proof, now)
            .expect("confirm link");

        // Deliver the response on the responder's return channel.
        fake.push_complete(Ok(Some(BASE64.encode(&encrypted_response))));

        // Second advance decrypts the response, adopts the identity, and navigates home.
        assert!(app.advance_device_link_responder_session());

        assert!(app.vauchi().has_identity(), "identity must be adopted");
        assert!(
            matches!(app.current_app_screen(), AppScreen::MyInfo),
            "expected MyInfo after adoption, got {:?}",
            app.current_app_screen()
        );
        assert_eq!(app.engine.current_screen().screen_id, "my_info");
        assert!(
            app.device_link_responder.is_none(),
            "responder must be dropped after adoption"
        );
    }
}
