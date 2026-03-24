// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-Based and Stateful Tests for Field Validation
//!
//! Uses proptest to verify invariants of the trust/validation system:
//! - Trust level monotonicity (count-based and weighted-score-based)
//! - Trust weight boundedness and ordering
//! - Stateful validation lifecycle (validate/revoke/block/unblock sequences)
//!
//! Traces to: _private/features/field_validation.feature @trust @trust-weight @blocked

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::social::{
    ProfileValidation, ValidationConfidence, ValidationStatus, ValidatorMeta,
    calculate_trust_weight,
};
use vauchi_core::{Contact, Identity, Vauchi};

// ============================================================
// Helper: numeric ordering for ValidationConfidence
// ============================================================

/// Maps ValidationConfidence to a u8 for monotonicity comparisons.
/// Unverified(0) < LowConfidence(1) < PartialConfidence(2) < HighConfidence(3)
fn trust_level_rank(level: ValidationConfidence) -> u8 {
    match level {
        ValidationConfidence::Unverified => 0,
        ValidationConfidence::LowConfidence => 1,
        ValidationConfidence::PartialConfidence => 2,
        ValidationConfidence::HighConfidence => 3,
    }
}

// ============================================================
// Property Tests: Trust Level Monotonicity
// ============================================================

proptest! {
    /// Trust level is monotonically non-decreasing with count.
    ///
    /// If count_a <= count_b, then ValidationConfidence::from_count(count_a)
    /// must be <= ValidationConfidence::from_count(count_b).
    #[test]
    fn prop_trust_level_monotonic_with_count(
        count_a in 0usize..100,
        count_b in 0usize..100
    ) {
        let level_a = ValidationConfidence::from_count(count_a);
        let level_b = ValidationConfidence::from_count(count_b);
        if count_a <= count_b {
            prop_assert!(
                trust_level_rank(level_a) <= trust_level_rank(level_b),
                "from_count({}) = {:?} (rank {}) should be <= from_count({}) = {:?} (rank {})",
                count_a, level_a, trust_level_rank(level_a),
                count_b, level_b, trust_level_rank(level_b),
            );
        }
    }

    /// Weighted score thresholds are monotonically non-decreasing.
    ///
    /// If score_a <= score_b, then ValidationConfidence::from_weighted_score(score_a)
    /// must be <= ValidationConfidence::from_weighted_score(score_b).
    #[test]
    fn prop_trust_level_monotonic_with_weighted_score(
        score_a in 0.0f32..10.0,
        score_b in 0.0f32..10.0
    ) {
        let level_a = ValidationConfidence::from_weighted_score(score_a);
        let level_b = ValidationConfidence::from_weighted_score(score_b);
        if score_a <= score_b {
            prop_assert!(
                trust_level_rank(level_a) <= trust_level_rank(level_b),
                "from_weighted_score({}) = {:?} (rank {}) should be <= from_weighted_score({}) = {:?} (rank {})",
                score_a, level_a, trust_level_rank(level_a),
                score_b, level_b, trust_level_rank(level_b),
            );
        }
    }
}

// ============================================================
// Property Tests: Trust Weight Bounds
// ============================================================

proptest! {
    /// Trust weight is always bounded in [0.0, 1.0] for any input.
    #[test]
    fn prop_trust_weight_bounded(age in 0u64..10000, verified in proptest::bool::ANY) {
        let weight = calculate_trust_weight(age, verified);
        prop_assert!(
            weight >= 0.0,
            "Trust weight must be >= 0.0, got {} for age={}, verified={}",
            weight, age, verified,
        );
        prop_assert!(
            weight <= 1.0,
            "Trust weight must be <= 1.0, got {} for age={}, verified={}",
            weight, age, verified,
        );
    }

    /// Fingerprint-verified contacts always have >= weight of unverified at same age.
    #[test]
    fn prop_verified_weight_gte_unverified(age in 0u64..10000) {
        let verified_weight = calculate_trust_weight(age, true);
        let unverified_weight = calculate_trust_weight(age, false);
        prop_assert!(
            verified_weight >= unverified_weight,
            "Verified weight ({}) must be >= unverified weight ({}) at age={}",
            verified_weight, unverified_weight, age,
        );
    }

    /// Trust weight is monotonically non-decreasing with age (for a fixed verification status).
    #[test]
    fn prop_trust_weight_monotonic_with_age(
        age_a in 0u64..10000,
        age_b in 0u64..10000,
        verified in proptest::bool::ANY
    ) {
        let weight_a = calculate_trust_weight(age_a, verified);
        let weight_b = calculate_trust_weight(age_b, verified);
        if age_a <= age_b {
            prop_assert!(
                weight_a <= weight_b,
                "Weight at age {} ({}) should be <= weight at age {} ({}) for verified={}",
                age_a, weight_a, age_b, weight_b, verified,
            );
        }
    }
}

// ============================================================
// Property Tests: ValidationStatus from_validations consistency
// ============================================================

proptest! {
    /// ValidationStatus count equals number of non-blocked validations
    /// matching the field value.
    #[test]
    fn prop_validation_status_count_matches_filtered(
        num_matching in 0usize..10,
        num_blocked in 0usize..5,
        num_mismatched in 0usize..5
    ) {
        let field_value = "test_value";
        let mut validations = Vec::new();
        let mut blocked_ids = HashSet::new();

        // Matching, non-blocked validations
        for i in 0..num_matching {
            let id = format!("validator_{}", i);
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                field_value,
                &id,
                1000000000 + i as u64,
                [0u8; 64],
            ));
        }

        // Matching, blocked validations
        for i in 0..num_blocked {
            let id = format!("blocked_{}", i);
            blocked_ids.insert(id.clone());
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                field_value,
                &id,
                1000000000 + i as u64,
                [0u8; 64],
            ));
        }

        // Non-matching field value validations (should be filtered out)
        for i in 0..num_mismatched {
            let id = format!("mismatch_{}", i);
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                "different_value",
                &id,
                1000000000 + i as u64,
                [0u8; 64],
            ));
        }

        let status = ValidationStatus::from_validations(
            &validations,
            field_value,
            None,
            &blocked_ids,
        );

        prop_assert_eq!(
            status.count, num_matching,
            "Count should equal number of matching non-blocked validations. \
             Got {} expected {}. matching={}, blocked={}, mismatched={}",
            status.count, num_matching, num_matching, num_blocked, num_mismatched,
        );

        // Blocked validators must never appear in validator_ids
        for blocked_id in &blocked_ids {
            prop_assert!(
                !status.validator_ids.contains(blocked_id),
                "Blocked validator {} must not appear in status.validator_ids",
                blocked_id,
            );
        }
    }

    /// validated_by_me is true iff my_id is among the non-blocked, matching validators.
    #[test]
    fn prop_validated_by_me_correct(
        my_validation_exists in proptest::bool::ANY,
        my_is_blocked in proptest::bool::ANY
    ) {
        let field_value = "email@test.com";
        let my_id = "my_validator_id";
        let mut validations = Vec::new();
        let mut blocked_ids = HashSet::new();

        if my_validation_exists {
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                field_value,
                my_id,
                1000000000,
                [0u8; 64],
            ));
        }

        if my_is_blocked {
            blocked_ids.insert(my_id.to_string());
        }

        let status = ValidationStatus::from_validations(
            &validations,
            field_value,
            Some(my_id),
            &blocked_ids,
        );

        let expected = my_validation_exists && !my_is_blocked;
        prop_assert_eq!(
            status.validated_by_me, expected,
            "validated_by_me should be {} (exists={}, blocked={})",
            expected, my_validation_exists, my_is_blocked,
        );
    }
}

// ============================================================
// Stateful Property Test: Validation Lifecycle Invariants
// ============================================================

/// Operations that can be applied to the validation system.
#[derive(Debug, Clone)]
enum ValidationOp {
    /// Create a signed validation from validator at index.
    Validate { validator_idx: usize },
    /// Revoke validation from validator at index.
    Revoke { validator_idx: usize },
    /// Block the validator at index.
    Block { validator_idx: usize },
    /// Unblock the validator at index.
    Unblock { validator_idx: usize },
}

/// Strategy for generating random validation operations.
fn arb_validation_op() -> impl Strategy<Value = ValidationOp> {
    prop_oneof![
        (0..5usize).prop_map(|i| ValidationOp::Validate { validator_idx: i }),
        (0..5usize).prop_map(|i| ValidationOp::Revoke { validator_idx: i }),
        (0..5usize).prop_map(|i| ValidationOp::Block { validator_idx: i }),
        (0..5usize).prop_map(|i| ValidationOp::Unblock { validator_idx: i }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Random sequences of validate/revoke/block/unblock maintain invariants:
    ///
    /// 1. Blocked validators never appear in validation status
    /// 2. Count matches number of non-blocked, active validators
    /// 3. Trust level is consistent with count
    #[test]
    fn prop_validation_lifecycle_invariants(
        ops in prop::collection::vec(arb_validation_op(), 1..30)
    ) {
        // Set up a Vauchi instance with 5 validator contacts
        let mut wb: Vauchi = Vauchi::in_memory().unwrap();
        wb.create_identity("Owner").unwrap();

        // Create the target contact whose field will be validated
        let target_key = [42u8; 32];
        let target_contact = Contact::from_exchange(
            target_key,
            ContactCard::new("Target"),
            SymmetricKey::generate(),
        );
        let target_id = target_contact.id().to_string();
        wb.add_contact(target_contact).unwrap();

        // Create 5 validator identities and add them as contacts
        let mut validator_identities = Vec::new();
        let mut validator_ids = Vec::new();
        for i in 0..5 {
            let identity = Identity::create(&format!("Validator {}", i));
            let contact = Contact::from_exchange(
                *identity.signing_public_key(),
                ContactCard::new(&format!("Validator {}", i)),
                SymmetricKey::generate(),
            );
            let vid = contact.id().to_string();
            wb.add_contact(contact).unwrap();
            validator_ids.push(vid);
            validator_identities.push(identity);
        }

        // Track local state for invariant checking
        let mut has_validated: HashSet<usize> = HashSet::new();
        let mut is_blocked: HashSet<usize> = HashSet::new();

        let field_id = "twitter";
        let field_value = "@target";

        for op in &ops {
            match op {
                ValidationOp::Validate { validator_idx } => {
                    let idx = *validator_idx;
                    if !has_validated.contains(&idx) {
                        // Create a signed validation directly and save it
                        let validation = ProfileValidation::create_signed(
                            &validator_identities[idx],
                            field_id,
                            field_value,
                            &target_id,
                        );
                        wb.storage().save_validation(&validation).unwrap();
                        has_validated.insert(idx);
                    }
                }
                ValidationOp::Revoke { validator_idx } => {
                    let idx = *validator_idx;
                    if has_validated.contains(&idx) {
                        wb.storage()
                            .delete_validation(&target_id, field_id, &validator_ids[idx])
                            .unwrap();
                        has_validated.remove(&idx);
                    }
                }
                ValidationOp::Block { validator_idx } => {
                    let idx = *validator_idx;
                    if !is_blocked.contains(&idx) {
                        wb.block_contact(&validator_ids[idx]).unwrap();
                        is_blocked.insert(idx);
                    }
                }
                ValidationOp::Unblock { validator_idx } => {
                    let idx = *validator_idx;
                    if is_blocked.contains(&idx) {
                        wb.unblock_contact(&validator_ids[idx]).unwrap();
                        is_blocked.remove(&idx);
                    }
                }
            }

            // === Check invariants after every operation ===

            let status = wb
                .get_field_validation_status(&target_id, field_id, field_value)
                .unwrap();

            // Invariant 1: Blocked validators never appear in validation status
            for blocked_idx in &is_blocked {
                prop_assert!(
                    !status.validator_ids.contains(&validator_ids[*blocked_idx]),
                    "Blocked validator {} must not appear in status after {:?}. \
                     status.validator_ids = {:?}, blocked = {:?}",
                    validator_ids[*blocked_idx], op, status.validator_ids, is_blocked,
                );
            }

            // Invariant 2: Count matches non-blocked active validators
            let expected_count = has_validated
                .iter()
                .filter(|idx| !is_blocked.contains(idx))
                .count();
            prop_assert_eq!(
                status.count, expected_count,
                "Count mismatch after {:?}. has_validated={:?}, is_blocked={:?}",
                op, has_validated, is_blocked,
            );

            // Invariant 3: Weighted trust level must be <= count-based trust level.
            // Weighted scoring can only reduce trust (newer/unverified contacts
            // have lower weight), never increase it beyond what the raw count gives.
            let count_based_level = ValidationConfidence::from_count(expected_count);
            prop_assert!(
                trust_level_rank(status.trust_level) <= trust_level_rank(count_based_level),
                "Weighted trust level {:?} (rank {}) must be <= count-based level {:?} (rank {}) after {:?}",
                status.trust_level, trust_level_rank(status.trust_level),
                count_based_level, trust_level_rank(count_based_level),
                op,
            );

            // Invariant 4: All non-blocked active validators appear in validator_ids
            for active_idx in &has_validated {
                if !is_blocked.contains(active_idx) {
                    prop_assert!(
                        status.validator_ids.contains(&validator_ids[*active_idx]),
                        "Active non-blocked validator {} should appear in status after {:?}. \
                         status.validator_ids = {:?}",
                        validator_ids[*active_idx], op, status.validator_ids,
                    );
                }
            }
        }
    }
}

// ============================================================
// Property Tests: Sybil Resistance
// ============================================================

proptest! {
    /// A validator can only validate a field once (duplicate detection).
    #[test]
    fn prop_sybil_resistance_prevents_duplicate(
        num_attempts in 2usize..10
    ) {
        use vauchi_core::social::check_sybil_resistance;

        let contact_id = "target_contact";
        let field_id = "email";
        let validator_id = "validator_1";

        // First validation creates one record
        let existing = vec![
            ProfileValidation::from_stored(
                &format!("{}:{}", contact_id, field_id),
                "test@example.com",
                validator_id,
                1000000000,
                [0u8; 64],
            ),
        ];

        // Subsequent attempts should be rejected
        for _ in 1..num_attempts {
            let allowed = check_sybil_resistance(
                contact_id,
                field_id,
                validator_id,
                &existing,
            );
            prop_assert!(
                !allowed,
                "Sybil resistance must reject duplicate validation after {} attempts",
                num_attempts,
            );
        }

        // A different validator should still be allowed
        let other_allowed = check_sybil_resistance(
            contact_id,
            field_id,
            "other_validator",
            &existing,
        );
        prop_assert!(
            other_allowed,
            "A different validator must be allowed to validate",
        );
    }
}

// ============================================================
// Property Tests: Blocked Contact Filtering
// ============================================================

proptest! {
    /// filter_blocked_validations removes exactly the blocked validators.
    #[test]
    fn prop_filter_blocked_precise(
        num_clean in 0usize..10,
        num_blocked in 0usize..10
    ) {
        use vauchi_core::social::filter_blocked_validations;

        let mut validations = Vec::new();
        let mut blocked = HashSet::new();

        for i in 0..num_clean {
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                "value",
                &format!("clean_{}", i),
                1000000000,
                [0u8; 64],
            ));
        }

        for i in 0..num_blocked {
            let id = format!("blocked_{}", i);
            blocked.insert(id.clone());
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                "value",
                &id,
                1000000000,
                [0u8; 64],
            ));
        }

        let filtered = filter_blocked_validations(&validations, &blocked);

        prop_assert_eq!(
            filtered.len(), num_clean,
            "Filtered list should contain only non-blocked validations. \
             Got {} expected {}",
            filtered.len(), num_clean,
        );

        // No blocked validator should remain
        for v in &filtered {
            prop_assert!(
                !blocked.contains(v.validator_id()),
                "Blocked validator {} should not be in filtered results",
                v.validator_id(),
            );
        }
    }
}

// ============================================================
// Property Tests: Weighted ValidationStatus consistency
// ============================================================

proptest! {
    /// When all validators are mature and fingerprint-verified,
    /// the weighted trust level should match the count-based level
    /// (since each gets weight 1.0).
    #[test]
    fn prop_weighted_status_mature_verified_matches_count_level(
        num_validators in 0usize..8
    ) {
        let field_value = "test@example.com";
        let mut validations = Vec::new();
        let mut validator_meta = HashMap::new();

        for i in 0..num_validators {
            let id = format!("validator_{}", i);
            validations.push(ProfileValidation::from_stored(
                "contact:field",
                field_value,
                &id,
                1000000000 + i as u64,
                [0u8; 64],
            ));
            validator_meta.insert(id, ValidatorMeta {
                contact_age_days: 365, // Well past maturity
                fingerprint_verified: true,
            });
        }

        let status = ValidationStatus::from_validations_weighted(
            &validations,
            field_value,
            None,
            &HashSet::new(),
            &validator_meta,
        );

        // Each mature verified validator has weight 1.0,
        // so weighted_score = num_validators as f32.
        let expected_weighted_level = ValidationConfidence::from_weighted_score(num_validators as f32);

        prop_assert_eq!(
            status.trust_level, expected_weighted_level,
            "With all mature verified validators, weighted level should match \
             from_weighted_score({}). Got {:?}, expected {:?}",
            num_validators, status.trust_level, expected_weighted_level,
        );
        prop_assert_eq!(
            status.count, num_validators,
            "Count must equal number of validators",
        );
    }
}
