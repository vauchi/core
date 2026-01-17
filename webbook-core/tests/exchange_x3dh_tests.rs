//! Tests for exchange::x3dh
//! Extracted from x3dh.rs

use webbook_core::*;
use webbook_core::exchange::*;

    #[test]
    fn test_keypair_generation() {
        let kp = X3DHKeyPair::generate();
        assert_eq!(kp.public_key().len(), 32);
    }

    #[test]
    fn test_keypair_from_bytes_roundtrip() {
        let kp1 = X3DHKeyPair::generate();
        let bytes = kp1.secret_bytes();
        let kp2 = X3DHKeyPair::from_bytes(bytes);

        assert_eq!(kp1.public_key(), kp2.public_key());
    }
