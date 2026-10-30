// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ordering metadata remains an unknown item to the released tolerant reader.

use super::{DeviceSyncError, MAX_SYNC_ITEMS_PER_BATCH, SyncItem};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SENTINEL: &str = "_VauchiDeviceSyncOrder";
pub(crate) const MAX_DEVICE_SYNC_BYTES: usize = 48 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderMetadata {
    version: u32,
    sender: [u8; 32],
    recipient: [u8; 32],
    sequences: Vec<Option<u64>>,
}

fn invalid() -> DeviceSyncError {
    DeviceSyncError::Deserialization("invalid authenticated device ordering metadata".into())
}

pub(crate) fn validate_sequences(sequences: &[Option<u64>]) -> Result<(), DeviceSyncError> {
    let mut previous = 0;
    for sequence in sequences.iter().flatten() {
        if *sequence <= previous {
            return Err(invalid());
        }
        previous = *sequence;
    }
    Ok(())
}

pub(crate) fn decode(
    bytes: &[u8],
    sender: &[u8; 32],
    recipient: &[u8; 32],
) -> Result<Vec<(SyncItem, Option<u64>)>, DeviceSyncError> {
    let mut values: Vec<Value> = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if values.len() > MAX_SYNC_ITEMS_PER_BATCH {
        return Err(invalid());
    }
    let markers: Vec<usize> = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| value.get(SENTINEL).map(|_| index))
        .collect();
    let sequences = if markers.is_empty() {
        vec![None; values.len()]
    } else {
        if markers != [0] {
            return Err(invalid());
        }
        let mut entries = values.into_iter();
        let Some(Value::Object(mut object)) = entries.next() else {
            return Err(invalid());
        };
        if object.len() != 1 {
            return Err(invalid());
        }
        let metadata_value = object.remove(SENTINEL).ok_or_else(invalid)?;
        let metadata: OrderMetadata =
            serde_json::from_value(metadata_value).map_err(|_| invalid())?;
        values = entries.collect();
        if metadata.version != 1
            || &metadata.sender != sender
            || &metadata.recipient != recipient
            || metadata.sequences.len() != values.len()
        {
            return Err(invalid());
        }
        validate_sequences(&metadata.sequences)?;
        metadata.sequences
    };
    Ok(values
        .into_iter()
        .zip(sequences)
        .filter_map(|(value, sequence)| {
            serde_json::from_value(value)
                .ok()
                .map(|item| (item, sequence))
        })
        .collect())
}

fn encode(
    items: &[Value],
    sequences: &[Option<u64>],
    sender: &[u8; 32],
    recipient: &[u8; 32],
) -> Result<Vec<u8>, DeviceSyncError> {
    let metadata = OrderMetadata {
        version: 1,
        sender: *sender,
        recipient: *recipient,
        sequences: sequences.to_vec(),
    };
    let mut values = Vec::with_capacity(items.len().checked_add(1).ok_or_else(invalid)?);
    values.push(json!({SENTINEL: metadata}));
    values.extend_from_slice(items);
    serde_json::to_vec(&values).map_err(|error| DeviceSyncError::Serialization(error.to_string()))
}

pub(crate) fn batches(
    items: &[SyncItem],
    sequences: &[Option<u64>],
    sender: &[u8; 32],
    recipient: &[u8; 32],
) -> Result<Vec<Vec<u8>>, DeviceSyncError> {
    if items.len() != sequences.len() {
        return Err(invalid());
    }
    validate_sequences(sequences)?;
    let mut output = Vec::new();
    let mut batch = Vec::new();
    let mut orders = Vec::new();
    for (item, sequence) in items.iter().zip(sequences) {
        let value = serde_json::to_value(item)
            .map_err(|error| DeviceSyncError::Serialization(error.to_string()))?;
        batch.push(value);
        orders.push(*sequence);
        let candidate = encode(&batch, &orders, sender, recipient)?;
        if batch.len() > 1
            && (candidate.len() > MAX_DEVICE_SYNC_BYTES || batch.len() >= MAX_SYNC_ITEMS_PER_BATCH)
        {
            let item = batch.pop().ok_or_else(invalid)?;
            let sequence = orders.pop().ok_or_else(invalid)?;
            output.push(encode(&batch, &orders, sender, recipient)?);
            batch.clear();
            orders.clear();
            batch.push(item);
            orders.push(sequence);
        }
    }
    if !batch.is_empty() {
        output.push(encode(&batch, &orders, sender, recipient)?);
    }
    Ok(output)
}
