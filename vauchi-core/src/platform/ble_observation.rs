// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

const MAX_SERVICE_UUIDS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BleObservationError {
    #[error("invalid native BLE device identifier")]
    InvalidDeviceId,
    #[error("BLE advertisement has more than 32 service UUIDs")]
    TooManyServices,
}

/// Bounded native observations, without decoded exchange state. UUID bytes
/// use network order (the order in the conventional hexadecimal UUID).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BleAdvertisementObservation {
    #[cfg_attr(feature = "schema-gen", schemars(length(min = 1, max = 128)))]
    id: String,
    rssi: i16,
    #[cfg_attr(feature = "schema-gen", schemars(length(max = 32)))]
    service_uuids: Vec<[u8; 16]>,
}

impl BleAdvertisementObservation {
    pub fn new(
        id: String,
        rssi: i16,
        service_uuids: Vec<[u8; 16]>,
    ) -> Result<Self, BleObservationError> {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(BleObservationError::InvalidDeviceId);
        }
        if service_uuids.len() > MAX_SERVICE_UUIDS {
            return Err(BleObservationError::TooManyServices);
        }
        Ok(Self {
            id,
            rssi,
            service_uuids,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn rssi(&self) -> i16 {
        self.rssi
    }
    pub fn service_uuids(&self) -> &[[u8; 16]] {
        &self.service_uuids
    }
}

impl<'de> Deserialize<'de> for BleAdvertisementObservation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            rssi: i16,
            #[serde(deserialize_with = "service_uuids")]
            service_uuids: Vec<[u8; 16]>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.id, raw.rssi, raw.service_uuids).map_err(de::Error::custom)
    }
}

fn service_uuids<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<[u8; 16]>, D::Error> {
    struct Uuids;
    impl<'de> Visitor<'de> for Uuids {
        type Value = Vec<[u8; 16]>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("at most 32 UUIDs, each exactly 16 bytes")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            let mut result = Vec::new();
            while let Some(uuid) = sequence.next_element::<[u8; 16]>()? {
                if result.len() == MAX_SERVICE_UUIDS {
                    return Err(de::Error::custom(BleObservationError::TooManyServices));
                }
                result.push(uuid);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_seq(Uuids)
}
