use vauchi_core::exchange::*;
use vauchi_core::*;

#[test]
fn test_lazy_frontend_skips_proximity() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session =
        ExchangeSession::new_initiator(alice_identity, alice_card, alice_proximity);

    alice_session.apply(ExchangeEvent::GenerateQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    // Bob's proximity verifier will fail
    let bob_proximity = MockProximityVerifier::failure();
    let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, bob_proximity);

    // 1. Bob scans Alice's QR
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // 2. Bob's frontend is "lazy" and doesn't call verify_proximity()
    // but tries to call perform_key_agreement() directly.

    let res = bob_session.apply(ExchangeEvent::PerformKeyAgreement);

    assert!(
        res.is_err(),
        "Should NOT be able to perform key agreement without proximity verification"
    );
}

#[test]
fn test_formalized_state_machine() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let mut alice_session = ExchangeSession::new_initiator(
        alice_identity,
        alice_card,
        MockProximityVerifier::success(),
    );

    // Test transition using apply
    alice_session.apply(ExchangeEvent::GenerateQR).unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingScan { .. }
    ));

    // Test invalid transition
    let res = alice_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(res.is_err());
    assert!(matches!(res.unwrap_err(), ExchangeError::InvalidState(_)));
}
