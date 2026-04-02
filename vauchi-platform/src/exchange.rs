// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Session Mobile Bindings
//!
//! Wraps vauchi-core's `ExchangeSession` state machine for mobile platforms.
//! Provides a callback interface for proximity verification and a UniFFI object
//! that mobile apps drive through the exchange flow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeCommand, ExchangeEvent, ExchangeHardwareEvent, ExchangeQR, ExchangeSession,
    ExchangeState, ManualConfirmationVerifier, ProximityError, ProximityVerifier, VerifierChain,
    VerifierMethod,
};

use crate::error::{LOCK_POISON_MSG, lock_or};
use vauchi_core::identity::Identity;

use crate::error::MobileError;

// === Callback Interface ===

/// Callback interface for platform-specific proximity verification.
///
/// Mobile apps (iOS/Android) implement this to provide proximity verification
/// using ultrasonic audio or other hardware-based mechanisms.
#[uniffi::export(callback_interface)]
pub trait MobileProximityHandler: Send + Sync {
    /// Perform single-direction proximity verification (legacy).
    ///
    /// challenge: 16 bytes from the QR code's audio_challenge field.
    /// timeout_ms: maximum time to wait in milliseconds.
    ///
    /// Returns empty string on success, error message on failure.
    fn verify_proximity(&self, challenge: Vec<u8>, timeout_ms: u64) -> String;

    /// Perform bidirectional proximity verification.
    ///
    /// Both devices must independently prove they can hear each other.
    /// The initiator emits first then listens; the responder listens first then emits.
    ///
    /// emit_challenge: 16 bytes to emit (from peer's QR audio_challenge).
    /// listen_challenge: 16 bytes to listen for (our challenge, sent via encrypted channel).
    /// timeout_ms: maximum time to wait in milliseconds.
    /// is_initiator: true if this device scanned the QR (emit first), false if displayed.
    ///
    /// Returns empty string on success, error message on failure.
    ///
    /// Default: falls back to single-direction (emit only) for backward compat.
    /// Mobile apps should override this to enforce both directions.
    fn verify_proximity_two_way(
        &self,
        emit_challenge: Vec<u8>,
        _listen_challenge: Vec<u8>,
        timeout_ms: u64,
        _is_initiator: bool,
    ) -> String {
        self.verify_proximity(emit_challenge, timeout_ms)
    }
}

// === ProximityBridge ===

/// Adapts a `MobileProximityHandler` callback to vauchi-core's `ProximityVerifier` trait.
pub(crate) struct ProximityBridge {
    handler: Arc<dyn MobileProximityHandler>,
}

impl ProximityVerifier for ProximityBridge {
    fn confidence_level(&self) -> vauchi_core::exchange::ProximityConfidence {
        vauchi_core::exchange::ProximityConfidence::High
    }

    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        // Safety-net: the mobile handler is invoked at the two_way level.
        // These stubs exist only for trait completeness. If verify_proximity_two_way
        // were accidentally removed, the default impl would call these stubs and
        // fail safely via Err(NotSupported) in the real handler path.
        Err(ProximityError::NotSupported)
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        Err(ProximityError::NotSupported)
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        false
    }

    fn verify_proximity(
        &self,
        challenge: &[u8; 16],
        timeout: Duration,
    ) -> Result<(), ProximityError> {
        let result = self
            .handler
            .verify_proximity(challenge.to_vec(), timeout.as_millis() as u64);
        if result.is_empty() {
            Ok(())
        } else {
            Err(ProximityError::DeviceError(result))
        }
    }

    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> Result<(), ProximityError> {
        let result = self.handler.verify_proximity_two_way(
            emit_challenge.to_vec(),
            listen_challenge.to_vec(),
            timeout.as_millis() as u64,
            is_initiator,
        );
        if result.is_empty() {
            Ok(())
        } else {
            Err(ProximityError::DeviceError(result))
        }
    }
}

// === ManualConfirmationBridge ===

/// Wraps `ManualConfirmationVerifier` for devices without audio hardware.
///
/// Created internally — no callback interface needed. The mobile app calls
/// `confirm_proximity()` on the session, which sets the confirmation flag
/// before the state machine checks it.
pub(crate) struct ManualConfirmationBridge {
    inner: Arc<ManualConfirmationVerifier>,
}

impl ManualConfirmationBridge {
    fn new() -> (Self, Arc<ManualConfirmationVerifier>) {
        let verifier = Arc::new(ManualConfirmationVerifier::new());
        (
            Self {
                inner: verifier.clone(),
            },
            verifier,
        )
    }
}

impl ProximityVerifier for ManualConfirmationBridge {
    fn confidence_level(&self) -> vauchi_core::exchange::ProximityConfidence {
        self.inner.confidence_level()
    }

    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.inner.emit_challenge(challenge)
    }

    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        self.inner.listen_for_response(timeout)
    }

    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        self.inner.verify_response(challenge, response)
    }

    fn verify_proximity(
        &self,
        challenge: &[u8; 16],
        timeout: Duration,
    ) -> Result<(), ProximityError> {
        self.inner.verify_proximity(challenge, timeout)
    }
}

// === Mobile-Friendly State Enum ===

/// Mobile-friendly exchange state (no raw bytes or core types).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileExchangeState {
    Idle,
    DisplayingQr {
        qr_data: String,
    },
    PeerScanned,
    AwaitingKeyAgreement,
    AwaitingCardExchange,
    Complete {
        contact_id: String,
        contact_name: String,
    },
    Failed {
        error: String,
    },
}

/// Status of BLE exchange availability on this device.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileBleExchangeStatus {
    /// BLE exchange is available (native transport configured)
    Available,
    /// BLE exchange not available — requires native Bluetooth implementation
    NotAvailable { reason: String },
}

// === Session Wrapper ===

/// Mobile exchange session wrapping the core `ExchangeSession` state machine.
///
/// Drives the exchange flow: generate/scan QR -> verify proximity -> key agreement -> complete.
/// Since `ExchangeSession` stores `Box<dyn ProximityVerifier>`, no enum dispatch is needed.
#[derive(uniffi::Object)]
pub struct MobileExchangeSession {
    inner: Mutex<ExchangeSession>,
    manual_verifier: Option<Arc<ManualConfirmationVerifier>>,
}

impl MobileExchangeSession {
    /// Create a new session with an optional manual verifier handle.
    pub fn new(
        session: ExchangeSession,
        manual_verifier: Option<Arc<ManualConfirmationVerifier>>,
    ) -> Self {
        MobileExchangeSession {
            inner: Mutex::new(session),
            manual_verifier,
        }
    }

    /// Extract the contact from a completed session (used by finalize_exchange).
    pub fn extract_contact(&self) -> Result<Contact, MobileError> {
        let inner = lock_or(&self.inner)?;
        match inner.state() {
            ExchangeState::Complete { contact } => Ok(contact.clone()),
            _ => Err(MobileError::ExchangeFailed(
                "Session not in Complete state — drive the state machine first".into(),
            )),
        }
    }
}

#[uniffi::export]
impl MobileExchangeSession {
    /// Get the current state of the exchange session.
    pub fn state(&self) -> MobileExchangeState {
        let Ok(inner) = self.inner.lock() else {
            return MobileExchangeState::Failed {
                error: LOCK_POISON_MSG.into(),
            };
        };
        match inner.state() {
            ExchangeState::Idle => MobileExchangeState::Idle,
            ExchangeState::DisplayingQr { our_qr } => MobileExchangeState::DisplayingQr {
                qr_data: format!("wb://{}", our_qr.to_data_string()),
            },
            ExchangeState::PeerScanned { .. } => MobileExchangeState::PeerScanned,
            ExchangeState::AwaitingKeyAgreement { .. } => MobileExchangeState::AwaitingKeyAgreement,
            ExchangeState::AwaitingCardExchange { .. } => MobileExchangeState::AwaitingCardExchange,
            ExchangeState::Complete { contact } => MobileExchangeState::Complete {
                contact_id: contact.id().to_string(),
                contact_name: contact.display_name().to_string(),
            },
            ExchangeState::Failed { error } => MobileExchangeState::Failed {
                error: format!("{:?}", error),
            },
            ExchangeState::AwaitingNfcTap => MobileExchangeState::Idle,
            ExchangeState::AwaitingBleConnection => MobileExchangeState::Idle,
            ExchangeState::AwaitingBleVerification { .. } => MobileExchangeState::Idle,
            _ => MobileExchangeState::Idle,
        }
    }

    /// Generate and display a QR code. Transitions Idle -> DisplayingQr.
    pub fn generate_qr(&self) -> Result<String, MobileError> {
        let mut inner = lock_or(&self.inner)?;
        inner
            .apply(ExchangeEvent::StartQR)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))?;

        // Return the QR data string
        inner
            .qr()
            .map(|qr| format!("wb://{}", qr.to_data_string()))
            .ok_or_else(|| MobileError::ExchangeFailed("QR not generated".into()))
    }

    /// Process a scanned QR code. Transitions DisplayingQr -> PeerScanned.
    pub fn process_qr(&self, qr_data: String) -> Result<(), MobileError> {
        let data_str = qr_data.strip_prefix("wb://").unwrap_or(&qr_data);
        let qr = ExchangeQR::from_data_string(data_str).map_err(|_| MobileError::InvalidQrCode)?;

        let mut inner = lock_or(&self.inner)?;
        inner
            .apply(ExchangeEvent::ProcessQR(qr))
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Signal that the other party scanned our QR. Transitions PeerScanned -> AwaitingKeyAgreement.
    pub fn they_scanned_our_qr(&self) -> Result<(), MobileError> {
        let mut inner = lock_or(&self.inner)?;
        inner
            .apply(ExchangeEvent::TheyScannedOurQR)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Perform key agreement. Transitions AwaitingKeyAgreement -> AwaitingCardExchange.
    pub fn perform_key_agreement(&self) -> Result<(), MobileError> {
        let mut inner = lock_or(&self.inner)?;
        inner
            .apply(ExchangeEvent::PerformKeyAgreement)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Complete the card exchange. Transitions AwaitingCardExchange -> Complete.
    ///
    /// The `their_card_name` is used to create a placeholder card for the contact.
    /// The real card will be received via relay sync.
    pub fn complete_card_exchange(&self, their_card_name: String) -> Result<(), MobileError> {
        let card = ContactCard::new(&their_card_name);
        let mut inner = lock_or(&self.inner)?;
        inner
            .apply(ExchangeEvent::CompleteExchange(card))
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Confirm physical proximity (manual verification).
    ///
    /// For manual sessions: sets the confirmation flag so the exchange
    /// can proceed. Call this after the user confirms they are physically
    /// present with the other party.
    ///
    /// For proximity sessions: no-op (auto-verified via audio hardware).
    pub fn confirm_proximity(&self) -> Result<(), MobileError> {
        if let Some(verifier) = &self.manual_verifier {
            verifier.confirm();
        }
        Ok(())
    }

    /// Check if the session has timed out.
    pub fn is_timed_out(&self) -> bool {
        let Ok(inner) = self.inner.lock() else {
            return true;
        };
        inner.is_timed_out()
    }

    /// Returns the peer's display name extracted from their QR code.
    ///
    /// Available after `process_qr()` has been called successfully.
    pub fn peer_display_name(&self) -> Option<String> {
        let Ok(inner) = self.inner.lock() else {
            return None;
        };
        inner.their_display_name().map(String::from)
    }

    /// Returns the proximity confidence from the last verification.
    ///
    /// Available after key agreement has been performed.
    pub fn verification_confidence(
        &self,
    ) -> crate::mobile_verifier_event::MobileProximityConfidence {
        let Ok(inner) = self.inner.lock() else {
            return vauchi_core::exchange::ProximityConfidence::Unknown.into();
        };
        inner.proximity_confidence().into()
    }

    /// Enable exchange debug logging.
    ///
    /// Production API: intended for the debug panel settings toggle.
    /// When enabled, captures timestamped events at each state transition
    /// (QR generation, scan, key agreement, proximity, completion/failure).
    /// Call once before `generate_qr()`. Idempotent.
    pub fn enable_debug_log(&self) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.enable_debug_log();
    }

    /// Returns the exchange debug log as JSONL, if enabled.
    ///
    /// Production API: intended for diagnostic export (share sheet, clipboard).
    pub fn get_exchange_debug_jsonl(&self) -> Option<String> {
        let Ok(inner) = self.inner.lock() else {
            return None;
        };
        inner.exchange_debug_log().map(|log| log.to_jsonl())
    }

    /// Returns the exchange debug log as Markdown, if enabled.
    ///
    /// Production API: intended for diagnostic export (share sheet, clipboard).
    pub fn get_exchange_debug_markdown(&self) -> Option<String> {
        let Ok(inner) = self.inner.lock() else {
            return None;
        };
        inner.exchange_debug_log().map(|log| log.to_markdown())
    }

    // ── ADR-031: Command/Event API ──────────────────────────────────

    /// Drain all pending commands from the session (ADR-031).
    ///
    /// Call after any state-advancing method (generate_qr, process_qr,
    /// perform_key_agreement, etc.) to get hardware commands that the
    /// mobile app should execute (display QR, start BLE scan, emit audio, etc.).
    ///
    /// Returns an empty list if no commands are pending.
    pub fn drain_pending_commands(&self) -> Vec<MobileExchangeCommand> {
        let Ok(mut inner) = self.inner.lock() else {
            return Vec::new();
        };
        inner
            .drain_commands()
            .into_iter()
            .map(MobileExchangeCommand::from)
            .collect()
    }

    /// Feed a hardware event back to the session (ADR-031).
    ///
    /// Call when the platform completes a hardware action (QR scanned,
    /// BLE data received, NFC tap, audio response, etc.). The session
    /// advances its state machine and may produce new commands.
    ///
    /// After calling this, use `drain_pending_commands()` to get response commands.
    pub fn apply_hardware_event(
        &self,
        event: MobileExchangeHardwareEvent,
    ) -> Result<(), MobileError> {
        lock_or(&self.inner)?
            .apply_hardware_event(event.into())
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Returns the event log from the last proximity verification.
    ///
    /// Returns an empty list before any verification has occurred.
    /// After key agreement, contains the chain's events (InProgress,
    /// Completed, MethodFailed, FallingBack, etc.).
    pub fn get_verification_events(
        &self,
    ) -> Vec<crate::mobile_verifier_event::MobileProximityVerifierEvent> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        inner
            .proximity_event_log()
            .map(|log| log.events().iter().cloned().map(Into::into).collect())
            .unwrap_or_default()
    }
}

// === ADR-031: Command/Event UniFFI Exports ===

/// Exchange command sent from core to the frontend (ADR-031).
///
/// Mobile apps match on these and dispatch to platform-specific APIs
/// (camera, BLE stack, NFC reader, audio subsystem).
#[derive(uniffi::Enum, Debug, Clone)]
pub enum MobileExchangeCommand {
    // QR
    QrDisplay {
        data: String,
    },
    QrRequestScan,
    // BLE
    BleStartAdvertising {
        service_uuid: String,
        payload: Vec<u8>,
    },
    BleStartScanning {
        service_uuid: String,
    },
    BleConnect {
        device_id: String,
    },
    BleWriteCharacteristic {
        uuid: String,
        data: Vec<u8>,
    },
    BleReadCharacteristic {
        uuid: String,
    },
    BleDisconnect,
    // NFC
    NfcActivate {
        payload: Vec<u8>,
    },
    NfcDeactivate,
    // Audio
    AudioEmitChallenge {
        data: Vec<u8>,
    },
    AudioListenForResponse {
        timeout_ms: u64,
    },
    AudioStop,
    // Accelerometer
    AccelerometerStart,
    AccelerometerStop,
    // Relay escrow
    RelayEscrowDeposit {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
        encrypted_card: Vec<u8>,
        ttl_seconds: u32,
    },
    RelayEscrowCheck {
        gate_hash: Vec<u8>,
        suggested_interval_ms: u32,
    },
    RelayEscrowRetrieve {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
    },
    // Link mode
    ShowShareSheet {
        url: String,
    },
}

impl From<ExchangeCommand> for MobileExchangeCommand {
    fn from(cmd: ExchangeCommand) -> Self {
        match cmd {
            ExchangeCommand::QrDisplay { data } => Self::QrDisplay { data },
            ExchangeCommand::QrRequestScan => Self::QrRequestScan,
            ExchangeCommand::BleStartAdvertising {
                service_uuid,
                payload,
            } => Self::BleStartAdvertising {
                service_uuid,
                payload,
            },
            ExchangeCommand::BleStartScanning { service_uuid } => {
                Self::BleStartScanning { service_uuid }
            }
            ExchangeCommand::BleConnect { device_id } => Self::BleConnect { device_id },
            ExchangeCommand::BleWriteCharacteristic { uuid, data } => {
                Self::BleWriteCharacteristic { uuid, data }
            }
            ExchangeCommand::BleReadCharacteristic { uuid } => Self::BleReadCharacteristic { uuid },
            ExchangeCommand::BleDisconnect => Self::BleDisconnect,
            ExchangeCommand::NfcActivate { payload } => Self::NfcActivate { payload },
            ExchangeCommand::NfcDeactivate => Self::NfcDeactivate,
            ExchangeCommand::AudioEmitChallenge { data } => Self::AudioEmitChallenge { data },
            ExchangeCommand::AudioListenForResponse { timeout_ms } => {
                Self::AudioListenForResponse { timeout_ms }
            }
            ExchangeCommand::AudioStop => Self::AudioStop,
            ExchangeCommand::AccelerometerStart => Self::AccelerometerStart,
            ExchangeCommand::AccelerometerStop => Self::AccelerometerStop,
            ExchangeCommand::RelayEscrowDeposit {
                gate_hash,
                slot_hash,
                encrypted_card,
                ttl_seconds,
            } => Self::RelayEscrowDeposit {
                gate_hash,
                slot_hash,
                encrypted_card,
                ttl_seconds,
            },
            ExchangeCommand::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms,
            } => Self::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms,
            },
            ExchangeCommand::RelayEscrowRetrieve {
                gate_hash,
                slot_hash,
            } => Self::RelayEscrowRetrieve {
                gate_hash,
                slot_hash,
            },
            ExchangeCommand::ShowShareSheet { url } => Self::ShowShareSheet { url },
            _ => Self::QrRequestScan,
        }
    }
}

/// Hardware event reported by the frontend back to core (ADR-031).
///
/// Mobile apps create these after executing a command (e.g., QR scanned,
/// BLE data received) and feed them back via `apply_hardware_event()`.
#[derive(uniffi::Enum, Debug, Clone)]
pub enum MobileExchangeHardwareEvent {
    // QR
    QrScanned {
        data: String,
    },
    // BLE
    BleDeviceDiscovered {
        id: String,
        rssi: i16,
        adv_data: Vec<u8>,
    },
    BleConnected {
        device_id: String,
    },
    BleCharacteristicRead {
        uuid: String,
        data: Vec<u8>,
    },
    BleCharacteristicNotified {
        uuid: String,
        data: Vec<u8>,
    },
    BleDisconnected {
        reason: String,
    },
    // NFC
    NfcDataReceived {
        data: Vec<u8>,
    },
    // Audio
    AudioResponseReceived {
        data: Vec<u8>,
    },
    // Accelerometer
    AccelerometerData {
        timestamp_ms: u64,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    },
    ImpactDetected {
        timestamp_ms: u64,
        magnitude_milli_g: i32,
    },
    // Relay escrow
    RelayEscrowReady {
        gate_hash: Vec<u8>,
    },
    RelayEscrowBlobReceived {
        gate_hash: Vec<u8>,
        blob: Vec<u8>,
    },
    RelayEscrowFailed {
        gate_hash: Vec<u8>,
        reason: String,
    },
    // Link mode
    LinkShared,
    LinkOpened {
        peer_public_key: Vec<u8>,
    },
    // Errors
    HardwareError {
        transport: String,
        error: String,
    },
    HardwareUnavailable {
        transport: String,
    },
}

impl From<MobileExchangeHardwareEvent> for ExchangeHardwareEvent {
    fn from(evt: MobileExchangeHardwareEvent) -> Self {
        match evt {
            MobileExchangeHardwareEvent::QrScanned { data } => Self::QrScanned { data },
            MobileExchangeHardwareEvent::BleDeviceDiscovered { id, rssi, adv_data } => {
                Self::BleDeviceDiscovered { id, rssi, adv_data }
            }
            MobileExchangeHardwareEvent::BleConnected { device_id } => {
                Self::BleConnected { device_id }
            }
            MobileExchangeHardwareEvent::BleCharacteristicRead { uuid, data } => {
                Self::BleCharacteristicRead { uuid, data }
            }
            MobileExchangeHardwareEvent::BleCharacteristicNotified { uuid, data } => {
                Self::BleCharacteristicNotified { uuid, data }
            }
            MobileExchangeHardwareEvent::BleDisconnected { reason } => {
                Self::BleDisconnected { reason }
            }
            MobileExchangeHardwareEvent::NfcDataReceived { data } => Self::NfcDataReceived { data },
            MobileExchangeHardwareEvent::AudioResponseReceived { data } => {
                Self::AudioResponseReceived { data }
            }
            MobileExchangeHardwareEvent::HardwareError { transport, error } => {
                Self::HardwareError { transport, error }
            }
            MobileExchangeHardwareEvent::HardwareUnavailable { transport } => {
                Self::HardwareUnavailable { transport }
            }
            MobileExchangeHardwareEvent::AccelerometerData {
                timestamp_ms,
                x_milli_g,
                y_milli_g,
                z_milli_g,
            } => Self::AccelerometerData {
                timestamp_ms,
                x_milli_g,
                y_milli_g,
                z_milli_g,
            },
            MobileExchangeHardwareEvent::ImpactDetected {
                timestamp_ms,
                magnitude_milli_g,
            } => Self::ImpactDetected {
                timestamp_ms,
                magnitude_milli_g,
            },
            MobileExchangeHardwareEvent::RelayEscrowReady { gate_hash } => {
                Self::RelayEscrowReady { gate_hash }
            }
            MobileExchangeHardwareEvent::RelayEscrowBlobReceived { gate_hash, blob } => {
                Self::RelayEscrowBlobReceived { gate_hash, blob }
            }
            MobileExchangeHardwareEvent::RelayEscrowFailed { gate_hash, reason } => {
                Self::RelayEscrowFailed { gate_hash, reason }
            }
            MobileExchangeHardwareEvent::LinkShared => Self::LinkShared,
            MobileExchangeHardwareEvent::LinkOpened { peer_public_key } => {
                Self::LinkOpened { peer_public_key }
            }
        }
    }
}

// === Factory Functions ===

/// Create a QR exchange session with proximity verification.
///
/// Wraps the proximity bridge in a single-entry `VerifierChain` so that
/// verification events are always available via `get_verification_events()`.
pub fn create_qr_exchange_proximity(
    identity: Identity,
    our_card: ContactCard,
    handler: Box<dyn MobileProximityHandler>,
) -> Arc<MobileExchangeSession> {
    let bridge = ProximityBridge {
        handler: Arc::from(handler),
    };
    let mut chain = VerifierChain::new();
    // TODO(method-label): Method label is hardcoded to Ultrasonic — MobileProximityHandler
    // may actually use BLE or another mechanism. Revisit when the handler interface
    // reports its actual method.
    chain.add(VerifierMethod::Ultrasonic, Box::new(bridge));
    let session = ExchangeSession::new_qr(identity, our_card, chain);
    Arc::new(MobileExchangeSession::new(session, None))
}

/// Create a QR exchange session with manual confirmation.
///
/// Wraps the manual bridge in a single-entry `VerifierChain` so that
/// verification events are always available via `get_verification_events()`.
pub fn create_qr_exchange_manual(
    identity: Identity,
    our_card: ContactCard,
) -> Arc<MobileExchangeSession> {
    let (bridge, verifier) = ManualConfirmationBridge::new();
    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::ManualConfirmation, Box::new(bridge));
    let session = ExchangeSession::new_qr(identity, our_card, chain);
    Arc::new(MobileExchangeSession::new(session, Some(verifier)))
}

/// Check if BLE exchange is available on this device.
///
/// Returns NotAvailable until native BLE transport is implemented
/// for each platform (CoreBluetooth on iOS, Android BLE on Android).
#[uniffi::export]
pub fn ble_exchange_status() -> MobileBleExchangeStatus {
    MobileBleExchangeStatus::NotAvailable {
        reason: "Native Bluetooth transport not yet implemented".into(),
    }
}

// === Tests ===

// INLINE_TEST_REQUIRED: tests use private ProximityBridge internals and create_qr_exchange_manual/proximity helpers
#[cfg(test)]
mod tests {
    use super::*;

    /// Mock proximity handler that always succeeds.
    struct SuccessHandler;
    impl MobileProximityHandler for SuccessHandler {
        fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
            String::new()
        }
        // verify_proximity_two_way uses default (falls back to verify_proximity)
    }

    /// Mock proximity handler that always fails.
    struct FailureHandler;
    impl MobileProximityHandler for FailureHandler {
        fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
            "Device too far away".to_string()
        }
        // verify_proximity_two_way uses default (falls back to verify_proximity)
    }

    // @scenario: contact_exchange:Successful QR code exchange with proximity
    #[test]
    fn test_proximity_bridge_success() {
        let handler = Arc::new(SuccessHandler);
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let challenge = [0xAA; 16];
        let result = bridge.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok(), "expected success");
    }

    // @scenario: contact_exchange:QR code exchange blocked without proximity
    #[test]
    fn test_proximity_bridge_failure() {
        let handler = Arc::new(FailureHandler);
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let challenge = [0xBB; 16];
        let result = bridge.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_err(), "expected error");
        match result.unwrap_err() {
            ProximityError::DeviceError(msg) => assert_eq!(msg, "Device too far away"),
            other => panic!("Expected DeviceError, got {:?}", other),
        }
    }

    // @scenario: contact_exchange:Two-way proximity delegates both challenges
    #[test]
    fn test_proximity_bridge_two_way_delegates_both_challenges() {
        use std::sync::atomic::{AtomicU32, Ordering};

        /// Handler that records which challenges it received.
        struct RecordingHandler {
            calls: AtomicU32,
        }
        impl MobileProximityHandler for RecordingHandler {
            fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
                String::new()
            }
            fn verify_proximity_two_way(
                &self,
                emit: Vec<u8>,
                listen: Vec<u8>,
                _timeout_ms: u64,
                _is_initiator: bool,
            ) -> String {
                self.calls.fetch_add(1, Ordering::Relaxed);
                // Verify both challenges are distinct and correct length
                assert_eq!(emit.len(), 16, "emit challenge must be 16 bytes");
                assert_eq!(listen.len(), 16, "listen challenge must be 16 bytes");
                assert_ne!(emit, listen, "emit and listen challenges must differ");
                String::new()
            }
        }

        let handler = Arc::new(RecordingHandler {
            calls: AtomicU32::new(0),
        });
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let emit = [0xAA; 16];
        let listen = [0xBB; 16];
        let result = bridge.verify_proximity_two_way(&emit, &listen, Duration::from_secs(5), true);
        assert!(result.is_ok(), "two-way verification should succeed");
        assert_eq!(
            handler.calls.load(Ordering::Relaxed),
            1,
            "handler.verify_proximity_two_way should be called exactly once"
        );
    }

    // @scenario: contact_exchange:Two-way proximity failure propagates
    #[test]
    fn test_proximity_bridge_two_way_failure() {
        let handler = Arc::new(FailureHandler);
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let emit = [0xAA; 16];
        let listen = [0xBB; 16];
        let result = bridge.verify_proximity_two_way(&emit, &listen, Duration::from_secs(5), false);
        assert!(result.is_err(), "two-way should propagate failure");
    }

    // Remaining session-level tests moved to tests/exchange_session_mobile_tests.rs
    // (they use only the public MobileExchangeSession API, not ProximityBridge internals)
}
