// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `devices` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_devices(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::IsPrimaryDevice => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                Ok(DomainCommandResult::Bool {
                    value: identity.device_info().device_index() == 0,
                })
            }
            DomainCommand::GetDeviceCount => {
                let storage = engine.vauchi().storage();
                let count = match storage.device().load_device_registry().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })? {
                    Some(r) => r.device_count() as u32,
                    None => 1,
                };
                Ok(DomainCommandResult::Count { value: count })
            }
            DomainCommand::GetDevices => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let storage = engine.vauchi().storage();

                let registry = match storage.device().load_device_registry().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })? {
                    Some(r) => r,
                    None => {
                        let device_info = identity.device_info();
                        return Ok(DomainCommandResult::Devices {
                            devices: vec![crate::types::MobileDeviceInfo {
                                device_index: device_info.device_index(),
                                device_name: device_info.device_name().to_string(),
                                is_current: true,
                                is_active: true,
                                public_key_prefix: hex::encode(&device_info.device_id()[..8]),
                                created_at: device_info.created_at(),
                            }],
                        });
                    }
                };

                let current_device_id = identity.device_info().device_id();
                let devices = registry
                    .all_devices()
                    .iter()
                    .enumerate()
                    .map(
                        |(idx, d): (usize, &vauchi_core::identity::RegisteredDevice)| {
                            crate::types::MobileDeviceInfo {
                                device_index: idx as u32,
                                device_name: d.device_name.clone(),
                                is_current: d.device_id == *current_device_id,
                                is_active: d.is_active(),
                                public_key_prefix: hex::encode(&d.device_id[..8]),
                                created_at: d.created_at,
                            }
                        },
                    )
                    .collect();
                Ok(DomainCommandResult::Devices { devices })
            }
            DomainCommand::UnlinkDevice { device_index } => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let storage = engine.vauchi().storage();

                let mut registry = match storage.device().load_device_registry().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })? {
                    Some(r) => r,
                    None => return Ok(DomainCommandResult::Bool { value: false }),
                };

                let devices = registry.all_devices();
                if device_index as usize >= devices.len() {
                    return Ok(DomainCommandResult::Bool { value: false });
                }

                let device_id = devices[device_index as usize].device_id;
                if device_id == *identity.device_info().device_id() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Cannot unlink the current device".into(),
                    });
                }

                let result = match registry.revoke_device(
                    &device_id,
                    identity.signing_keypair(),
                    storage.clock().unix_seconds(),
                ) {
                    Ok(()) => {
                        storage
                            .device()
                            .save_device_registry(&registry)
                            .map_err(|e| MobileError::StorageError {
                                detail: e.to_string(),
                            })?;
                        true
                    }
                    Err(_) => false,
                };

                engine.invalidate_screen(&AppScreen::DeviceManagement);
                engine.invalidate_screen(&AppScreen::DeviceLinking);
                Ok(DomainCommandResult::Bool { value: result })
            }
            DomainCommand::GenerateDeviceLinkQr => {
                use vauchi_core::exchange::device_link::DeviceLinkQR;

                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;

                let qr = DeviceLinkQR::generate(
                    identity,
                    vauchi_core::clock::SystemClock::shared().unix_seconds(),
                );
                Ok(DomainCommandResult::DeviceLinkData {
                    data: crate::types::MobileDeviceLinkData {
                        qr_data: qr.to_data_string(),
                        identity_public_key: hex::encode(identity.signing_public_key()),
                        timestamp: qr.timestamp(),
                        expires_at: qr.expires_at(),
                    },
                })
            }
            DomainCommand::ParseDeviceLinkQr { qr_data } => {
                use vauchi_core::exchange::device_link::DeviceLinkQR;

                let qr = DeviceLinkQR::from_data_string(&qr_data).map_err(|_| {
                    MobileError::InvalidInput {
                        field: "qr".into(),
                        detail: "Invalid QR code".into(),
                    }
                })?;

                Ok(DomainCommandResult::DeviceLinkInfo {
                    info: crate::types::MobileDeviceLinkInfo {
                        identity_public_key: hex::encode(qr.identity_public_key()),
                        timestamp: qr.timestamp(),
                        is_expired: qr
                            .is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
                    },
                })
            }
            DomainCommand::EncodeMultipartQr { data } => {
                let frames = crate::multipart_qr::encode_multipart(&data, 1800);
                Ok(DomainCommandResult::Strings { values: frames })
            }

            // ── Certificate pinning (B7 batch 21) ──
            other => unreachable!("non-devices command {other:?} routed to devices dispatcher"),
        }
    }
}
