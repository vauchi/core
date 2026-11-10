// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The plaintext each encrypted column is filled with.

/// Plaintext fixtures keyed by `(table, column)`. Each plaintext is
/// distinct so a swap bug between columns would be caught.
pub(super) fn fixtures() -> Vec<(&'static str, &'static str, &'static [u8])> {
    vec![
        ("contacts", "card_encrypted", b"{\"name\":\"Bob\"}" as &[u8]),
        (
            "contact_device_registries",
            "broadcast_encrypted",
            b"signed-registry-broadcast-json",
        ),
        (
            "contact_device_key_pins",
            "pins_encrypted",
            b"[[[1,1],[2,2]]]",
        ),
        (
            "safety_alert_facts",
            "signed_payload_encrypted",
            b"signed-duress-alert-payload",
        ),
        (
            "contacts",
            "shared_key_encrypted",
            b"shared-key-32-bytes-padding!!!ab",
        ),
        ("contacts", "personal_notes_encrypted", b"loves dogs"),
        ("contacts", "avatar_encrypted", b"AVATARBYTES1"),
        (
            "contacts",
            "cek_encrypted",
            b"contact-encryption-key-32bytes!!",
        ),
        ("contacts", "visibility_rules_encrypted", b"{\"hidden\":[]}"),
        ("contacts", "nickname_encrypted", b"Bobby"),
        ("contacts", "custom_avatar_encrypted", b"CUSTOMAVATAR"),
        ("contacts", "last_sent_display_name_encrypted", b"Ada"),
        (
            "contacts",
            "relay_url_encrypted",
            b"https://relay.example.org",
        ),
        ("identity", "backup_data_encrypted", b"backup-blob"),
        ("identity", "password_hash_encrypted", b"argon2id$..."),
        ("identity", "duress_hash_encrypted", b"argon2id$duress"),
        (
            "contact_ratchets",
            "ratchet_state_encrypted",
            b"ratchet-state-blob",
        ),
        ("contact_field_notes", "note_encrypted", b"phone is work"),
        ("own_card", "card_json_encrypted", b"{\"name\":\"Alice\"}"),
        ("mask_seed", "seed_encrypted", &[0x5a; 32]),
        ("decoy_ids", "display_name_encrypted", b"Decoy"),
        ("public_values", "signing_public_key_encrypted", &[0x7b; 32]),
        ("public_values", "display_name_encrypted", b"Alice"),
        (
            "device_registry",
            "registry_json_encrypted",
            b"{\"devices\":[]}",
        ),
        (
            "device_sync_state",
            "state_json_encrypted",
            b"{\"vector\":1}",
        ),
        ("visibility_labels", "contacts_json_encrypted", b"[\"c1\"]"),
        (
            "visibility_labels",
            "visible_fields_json_encrypted",
            b"[\"name\"]",
        ),
        ("visibility_labels", "name_encrypted", b"Family"),
        (
            "visibility_labels",
            "display_name_override_encrypted",
            b"Custom Family",
        ),
        ("device_info", "device_info_encrypted", b"{\"id\":\"dev1\"}"),
        ("version_vector", "vector_json_encrypted", b"{\"v\":7}"),
        (
            "receive_checkpoints",
            "checkpoint_json_encrypted",
            b"{\"day\":20000}",
        ),
        (
            "sync_field_timestamps",
            "timestamps_json_encrypted",
            b"{\"field:email\":1700000000000}",
        ),
        (
            "contact_sync_timestamps",
            "last_sync_at_encrypted",
            b"\x00\x00\x00\x00\x68\x4F\x12\x34",
        ),
        ("pending_updates", "payload_encrypted", b"pending-payload"),
        ("retry_entries", "payload_encrypted", b"retry-payload"),
        (
            "device_sync_checkpoints",
            "items_json_encrypted",
            b"[\"item1\"]",
        ),
        ("recovery_responses", "response_encrypted", b"accept"),
        (
            "deletion_state",
            "state_json_encrypted",
            b"{\"state\":\"scheduled\"}",
        ),
        (
            "sync_checkpoints",
            "state_json_encrypted",
            b"{\"checkpoint\":1}",
        ),
        (
            "field_validations",
            "field_value_encrypted",
            b"alice@example.com",
        ),
        ("field_validations", "signature_encrypted", b"sig-bytes"),
        (
            "ux_state",
            "aha_tracker_json_encrypted",
            b"{\"first_login\":true}",
        ),
        (
            "ux_state",
            "demo_contact_json_encrypted",
            b"{\"demo\":\"alice\"}",
        ),
        (
            "ux_state",
            "onboarding_progress_encrypted",
            b"{\"step\":\"backup\"}",
        ),
        ("ux_state", "backup_reminder_encrypted", b"{\"days\":7}"),
        (
            "ux_state",
            "settings_flags_encrypted",
            b"{\"reduce_motion\":true}",
        ),
        ("ux_state", "relay_url_encrypted", b"https://relay.example"),
        ("ux_state", "relay_anchor_encrypted", &[0x5a; 32]),
        ("ux_state", "user_presence_digest_encrypted", &[0x3c; 32]),
        (
            "ux_state",
            "own_card_repropagate_encrypted",
            b"{\"needs\":true}",
        ),
        (
            "ux_state",
            "exchange_defaults_encrypted",
            b"{\"mode\":\"qr\"}",
        ),
        (
            "contacts",
            "exchange_location_encrypted",
            b"{\"place\":\"cafe\"}",
        ),
        ("tags", "name_encrypted", b"climbing"),
        ("places", "data_encrypted", b"{\"name\":\"Home\"}"),
        // The duress session's twins (vauchi/private#813).
        ("audit_log", "details_encrypted", b"audit-details"),
        (
            "emergency_config",
            "trusted_contact_ids_encrypted",
            b"[\"trusted-1\"]",
        ),
        ("emergency_config", "message_encrypted", b"emergency-msg"),
        (
            "recovery_settings",
            "settings_encrypted",
            b"{\"method\":\"social\"}",
        ),
        (
            "exchange_states",
            "encrypted_blob",
            b"{\"state\":\"link-pending\"}",
        ),
        (
            "contact_shared_avatars",
            "avatar_encrypted",
            b"SHAREDAVATAR",
        ),
        (
            "recovery_progress",
            "progress_encrypted",
            b"{\"vouchers\":0}",
        ),
    ]
}

/// Read a fixture by table+column.
pub(super) fn fx(table: &str, column: &str) -> Vec<u8> {
    fixtures()
        .into_iter()
        .find(|(t, c, _)| *t == table && *c == column)
        .map(|(_, _, v)| v.to_vec())
        .unwrap_or_else(|| panic!("missing fixture for {}.{}", table, column))
}
