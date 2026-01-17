//! Tests for exchange::proximity
//! Extracted from proximity.rs

use webbook_core::*;
use std::time::Duration;
use webbook_core::exchange::*;

    #[test]
    fn test_mock_proximity_success() {
        let verifier = MockProximityVerifier::success();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_proximity_failure() {
        let verifier = MockProximityVerifier::failure();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_proximity_timeout() {
        let verifier = MockProximityVerifier::timeout();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(matches!(result, Err(ProximityError::Timeout)));
    }

    #[test]
    fn test_mock_records_challenges() {
        let verifier = MockProximityVerifier::success();
        let challenge1 = [1u8; 16];
        let challenge2 = [2u8; 16];

        verifier.emit_challenge(&challenge1).unwrap();
        verifier.emit_challenge(&challenge2).unwrap();

        let emitted = verifier.emitted_challenges();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0], challenge1);
        assert_eq!(emitted[1], challenge2);
    }

    #[test]
    fn test_manual_confirmation() {
        let verifier = ManualConfirmationVerifier::new();
        let challenge = [0u8; 16];

        // Before confirmation, should fail
        assert!(!verifier.is_confirmed());

        // After confirmation, should succeed
        verifier.confirm();
        assert!(verifier.is_confirmed());

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }
