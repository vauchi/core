//! Tests for storage validation operations
//!
//! Coverage tests for storage/validation.rs

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::social::ProfileValidation;
use vauchi_core::storage::Storage;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_validation(
    contact_id: &str,
    field_name: &str,
    validator_id: &str,
) -> ProfileValidation {
    let full_field_id = format!("{}:{}", contact_id, field_name);
    let signature = [0u8; 64];
    ProfileValidation::from_stored(
        &full_field_id,
        "test@example.com",
        validator_id,
        1234567890,
        signature,
    )
}

#[test]
fn test_save_and_load_validation() {
    let storage = create_test_storage();
    let validation = create_test_validation("contact123", "email", "validator456");

    // Save validation
    storage
        .save_validation(&validation)
        .expect("Should save validation");

    // Load validations for field
    let loaded = storage
        .load_validations_for_field("contact123", "email")
        .expect("Should load validations");

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].field_value(), "test@example.com");
    assert_eq!(loaded[0].validator_id(), "validator456");
}

#[test]
fn test_count_validations_for_field() {
    let storage = create_test_storage();

    // Initially no validations
    let count = storage
        .count_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(count, 0);

    // Add a validation
    let validation = create_test_validation("contact123", "email", "validator1");
    storage.save_validation(&validation).unwrap();

    let count = storage
        .count_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(count, 1);

    // Add another validation from different validator
    let validation2 = create_test_validation("contact123", "email", "validator2");
    storage.save_validation(&validation2).unwrap();

    let count = storage
        .count_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_has_validated() {
    let storage = create_test_storage();
    let validation = create_test_validation("contact123", "email", "validator456");

    // Initially not validated
    assert!(!storage
        .has_validated("contact123", "email", "validator456")
        .unwrap());

    storage.save_validation(&validation).unwrap();

    // Now validated
    assert!(storage
        .has_validated("contact123", "email", "validator456")
        .unwrap());

    // Different validator hasn't validated
    assert!(!storage
        .has_validated("contact123", "email", "other_validator")
        .unwrap());
}

#[test]
fn test_delete_validation() {
    let storage = create_test_storage();
    let validation = create_test_validation("contact123", "email", "validator456");

    storage.save_validation(&validation).unwrap();
    assert_eq!(
        storage
            .count_validations_for_field("contact123", "email")
            .unwrap(),
        1
    );

    // Delete the validation
    let deleted = storage
        .delete_validation("contact123", "email", "validator456")
        .unwrap();
    assert!(deleted);

    // Should be gone
    assert_eq!(
        storage
            .count_validations_for_field("contact123", "email")
            .unwrap(),
        0
    );

    // Deleting again returns false
    let deleted_again = storage
        .delete_validation("contact123", "email", "validator456")
        .unwrap();
    assert!(!deleted_again);
}

#[test]
fn test_delete_validations_for_field() {
    let storage = create_test_storage();

    // Add multiple validations for same field
    let v1 = create_test_validation("contact123", "email", "validator1");
    let v2 = create_test_validation("contact123", "email", "validator2");
    let v3 = create_test_validation("contact123", "phone", "validator1");

    storage.save_validation(&v1).unwrap();
    storage.save_validation(&v2).unwrap();
    storage.save_validation(&v3).unwrap();

    // Delete all email validations
    let deleted_count = storage
        .delete_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(deleted_count, 2);

    // Email validations gone
    assert_eq!(
        storage
            .count_validations_for_field("contact123", "email")
            .unwrap(),
        0
    );

    // Phone validation still there
    assert_eq!(
        storage
            .count_validations_for_field("contact123", "phone")
            .unwrap(),
        1
    );
}

#[test]
fn test_load_validations_by_validator() {
    let storage = create_test_storage();

    // Add validations from same validator for different contacts/fields
    let v1 = create_test_validation("contact1", "email", "my_validator");
    let v2 = create_test_validation("contact2", "phone", "my_validator");
    let v3 = create_test_validation("contact1", "email", "other_validator");

    storage.save_validation(&v1).unwrap();
    storage.save_validation(&v2).unwrap();
    storage.save_validation(&v3).unwrap();

    // Load my validations
    let my_validations = storage
        .load_validations_by_validator("my_validator")
        .unwrap();

    assert_eq!(my_validations.len(), 2);

    // All should have my validator ID
    for v in &my_validations {
        assert_eq!(v.validator_id(), "my_validator");
    }
}

#[test]
fn test_validation_upsert() {
    let storage = create_test_storage();

    // Save initial validation
    let v1 = ProfileValidation::from_stored(
        "contact123:email",
        "old@example.com",
        "validator456",
        1000,
        [0u8; 64],
    );
    storage.save_validation(&v1).unwrap();

    // Save updated validation (same contact, field, validator)
    let v2 = ProfileValidation::from_stored(
        "contact123:email",
        "new@example.com",
        "validator456",
        2000,
        [1u8; 64],
    );
    storage.save_validation(&v2).unwrap();

    // Should still be only 1 validation
    let count = storage
        .count_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(count, 1);

    // Should have the updated value
    let loaded = storage
        .load_validations_for_field("contact123", "email")
        .unwrap();
    assert_eq!(loaded[0].field_value(), "new@example.com");
}
