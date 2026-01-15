//! BLE Proximity Verification (Stubs)
//!
//! Bluetooth Low Energy proximity verification for mobile platforms.
//! This module provides trait definitions and mock implementations
//! for future mobile SDK integration.

use std::time::Duration;
use super::{ProximityVerifier, ProximityError};

/// A discovered BLE device.
#[derive(Debug, Clone)]
pub struct BLEDevice {
    /// Unique device identifier
    pub id: String,
    /// Device name (if advertised)
    pub name: Option<String>,
    /// Signal strength in dBm
    pub rssi: i16,
    /// WebBook exchange token (from advertisement)
    pub exchange_token: Option<[u8; 32]>,
}

impl BLEDevice {
    /// Creates a new BLE device.
    pub fn new(id: &str, rssi: i16) -> Self {
        BLEDevice {
            id: id.to_string(),
            name: None,
            rssi,
            exchange_token: None,
        }
    }

    /// Creates a device with a name.
    pub fn with_name(id: &str, name: &str, rssi: i16) -> Self {
        BLEDevice {
            id: id.to_string(),
            name: Some(name.to_string()),
            rssi,
            exchange_token: None,
        }
    }

    /// Sets the exchange token.
    pub fn with_exchange_token(mut self, token: [u8; 32]) -> Self {
        self.exchange_token = Some(token);
        self
    }
}

/// BLE proximity verification trait.
///
/// Extends the base `ProximityVerifier` with BLE-specific capabilities
/// for discovering and measuring distance to nearby devices.
pub trait BLEProximityVerifier: ProximityVerifier {
    /// Discovers nearby BLE devices advertising WebBook exchange.
    fn discover_nearby(&self, timeout: Duration) -> Result<Vec<BLEDevice>, ProximityError>;

    /// Estimates distance to a device based on RSSI.
    ///
    /// Returns estimated distance in meters.
    fn estimate_distance(&self, device: &BLEDevice) -> Result<f32, ProximityError>;

    /// Checks if a device is within the specified range.
    fn is_within_range(&self, device: &BLEDevice, max_meters: f32) -> bool {
        self.estimate_distance(device)
            .map(|d| d <= max_meters)
            .unwrap_or(false)
    }

    /// Verifies proximity to a specific device.
    ///
    /// Returns Ok if the device is within acceptable range (default 2 meters).
    fn verify_device_proximity(&self, device: &BLEDevice) -> Result<(), ProximityError> {
        const DEFAULT_MAX_DISTANCE: f32 = 2.0;

        if self.is_within_range(device, DEFAULT_MAX_DISTANCE) {
            Ok(())
        } else {
            Err(ProximityError::TooFar)
        }
    }
}

/// Mock BLE verifier for testing.
pub struct MockBLEVerifier {
    /// Pre-configured nearby devices.
    pub devices: Vec<BLEDevice>,
    /// Simulated distance for all devices.
    pub simulated_distance: f32,
    /// Whether operations should succeed.
    pub should_succeed: bool,
}

impl MockBLEVerifier {
    /// Creates a mock verifier with nearby devices at the given distance.
    pub fn new(devices: Vec<BLEDevice>, distance: f32) -> Self {
        MockBLEVerifier {
            devices,
            simulated_distance: distance,
            should_succeed: true,
        }
    }

    /// Creates a mock verifier that always succeeds with default devices.
    pub fn success_at_distance(distance: f32) -> Self {
        let device = BLEDevice::new("mock-device-1", -50);
        MockBLEVerifier {
            devices: vec![device],
            simulated_distance: distance,
            should_succeed: true,
        }
    }

    /// Creates a mock verifier that always fails.
    pub fn failure() -> Self {
        MockBLEVerifier {
            devices: vec![],
            simulated_distance: 100.0,
            should_succeed: false,
        }
    }
}

impl ProximityVerifier for MockBLEVerifier {
    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        if self.should_succeed {
            Ok(())
        } else {
            Err(ProximityError::DeviceError("Mock failure".into()))
        }
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.should_succeed {
            Ok(vec![0u8; 16])
        } else {
            Err(ProximityError::Timeout)
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        self.should_succeed
    }
}

impl BLEProximityVerifier for MockBLEVerifier {
    fn discover_nearby(&self, _timeout: Duration) -> Result<Vec<BLEDevice>, ProximityError> {
        if self.should_succeed {
            Ok(self.devices.clone())
        } else {
            Err(ProximityError::DeviceError("BLE discovery failed".into()))
        }
    }

    fn estimate_distance(&self, _device: &BLEDevice) -> Result<f32, ProximityError> {
        if self.should_succeed {
            Ok(self.simulated_distance)
        } else {
            Err(ProximityError::DeviceError("Distance estimation failed".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ble_discover_nearby_devices() {
        let devices = vec![
            BLEDevice::with_name("device-1", "Alice's Phone", -45),
            BLEDevice::with_name("device-2", "Bob's Phone", -60),
        ];
        let verifier = MockBLEVerifier::new(devices.clone(), 1.5);

        let discovered = verifier.discover_nearby(Duration::from_secs(5)).unwrap();

        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].name.as_deref(), Some("Alice's Phone"));
        assert_eq!(discovered[1].name.as_deref(), Some("Bob's Phone"));
    }

    #[test]
    fn test_ble_accept_within_2_meters() {
        let verifier = MockBLEVerifier::success_at_distance(1.5); // 1.5 meters away
        let device = &verifier.devices[0];

        assert!(verifier.is_within_range(device, 2.0));
        assert!(verifier.verify_device_proximity(device).is_ok());
    }

    #[test]
    fn test_ble_reject_if_too_far() {
        let verifier = MockBLEVerifier::success_at_distance(5.0); // 5 meters away
        let device = &verifier.devices[0];

        assert!(!verifier.is_within_range(device, 2.0));

        let result = verifier.verify_device_proximity(device);
        assert!(matches!(result, Err(ProximityError::TooFar)));
    }

    #[test]
    fn test_ble_device_with_exchange_token() {
        let token = [42u8; 32];
        let device = BLEDevice::new("test-id", -50)
            .with_exchange_token(token);

        assert_eq!(device.exchange_token, Some(token));
    }

    #[test]
    fn test_ble_verifier_failure() {
        let verifier = MockBLEVerifier::failure();

        let result = verifier.discover_nearby(Duration::from_secs(1));
        assert!(result.is_err());
    }
}
