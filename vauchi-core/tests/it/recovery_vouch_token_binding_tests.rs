// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian vouches only with a token that names the claim's old key as
//! designator and the guardian itself; any other entry that happens to
//! open is not its designation (vauchi/private#930).

use base64::{Engine, engine::general_purpose::STANDARD};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::recovery::{RecoveryClaim, sealed_box};
use vauchi_core::{Vauchi, VauchiConfig, VauchiError};

use crate::common::mock_relay::{CannedResponse, MockRelay};

struct Helper {
    vauchi: Vauchi,
    _dir: tempfile::TempDir,
    alice: SigningKeyPair,
    alice_id: String,
}

fn helper_of_alice(relay: &MockRelay) -> Helper {
    let mut config = VauchiConfig::default().with_relay_url(relay.url());
    config.ohttp.allow_direct = true;
    config.relay.connect_timeout_ms = 2_000;
    let dir = tempfile::tempdir().unwrap();
    config.storage_path = dir.path().join("helper.db");
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Bob").unwrap();
    let alice = SigningKeyPair::generate();
    let contact = Contact::from_exchange(
        *alice.public_key().as_bytes(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    let alice_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    Helper {
        vauchi,
        _dir: dir,
        alice,
        alice_id,
    }
}

impl Helper {
    fn public_key(&self) -> vauchi_core::crypto::PublicKey {
        vauchi_core::crypto::PublicKey::from_bytes(
            *self.vauchi.identity().unwrap().signing_public_key(),
        )
    }

    /// Seals `token` the way `upload_guardian_entries` does: to the
    /// Montgomery form of the helper's signing key.
    fn relay_entry_for_me(&self, token: &GuardianToken) -> String {
        let verifying =
            ed25519_dalek::VerifyingKey::from_bytes(self.public_key().as_bytes()).unwrap();
        let x25519 = x25519_dalek::PublicKey::from(verifying.to_montgomery().to_bytes());
        let sealed = sealed_box::seal(&token.to_bytes(), &x25519).unwrap();
        format!(
            r#"{{"status":"ok","guardians":[{{"data":"{}"}}]}}"#,
            STANDARD.encode(sealed)
        )
    }

    fn vouch_with_relay_entry(&self, relay: &MockRelay, entry: &str) -> Result<(), VauchiError> {
        relay.queue(
            "guardian_query",
            CannedResponse::ok_json(entry.as_bytes().to_vec()),
        );
        let now = self.vauchi.clock().unix_seconds();
        let claim = RecoveryClaim::new(
            self.alice.public_key().as_bytes(),
            SigningKeyPair::generate().public_key().as_bytes(),
            now,
        );
        self.vauchi
            .vouch_for_claim(&claim, &self.alice_id)
            .map(|_| ())
    }
}

// @scenario: contact_recovery :: A guardian vouches only with a token naming the claim and itself
#[test]
fn a_guardian_vouches_only_with_a_token_naming_the_claim_and_itself() {
    let relay = MockRelay::start();
    let bob = helper_of_alice(&relay);
    let someone_else = SigningKeyPair::generate();
    let now = bob.vauchi.clock().unix_seconds();
    let alices_token = GuardianToken::create(&bob.alice, bob.public_key(), now);
    let someone_elses_token = GuardianToken::create(&someone_else, bob.public_key(), now);
    let token_for_another_guardian =
        GuardianToken::create(&bob.alice, someone_else.public_key(), now);

    let with_someone_elses =
        bob.vouch_with_relay_entry(&relay, &bob.relay_entry_for_me(&someone_elses_token));
    let with_another_guardians =
        bob.vouch_with_relay_entry(&relay, &bob.relay_entry_for_me(&token_for_another_guardian));
    let with_alices = bob.vouch_with_relay_entry(&relay, &bob.relay_entry_for_me(&alices_token));

    assert!(
        matches!(with_someone_elses, Err(VauchiError::InvalidState(_))),
        "a token designated by another key is not Alice's, got {with_someone_elses:?}"
    );
    assert!(
        matches!(with_another_guardians, Err(VauchiError::InvalidState(_))),
        "a token naming another guardian is not mine, got {with_another_guardians:?}"
    );
    assert!(with_alices.is_ok(), "{with_alices:?}");
}
