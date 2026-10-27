// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::delivery::EventSender;
use crate::ports::{Discovery, HardwareFailure};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::future::Future;
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use uuid::Uuid;
use vauchi_core::platform::executor::DeliveryError;
use vauchi_core::platform::{BleAdvertisementObservation, BleServiceUuid, Event};
use zbus::message::Type;
use zbus::{Connection, MatchRule, Message, MessageStream, OwnedObjectPath, OwnedVariant};

const SYSTEM_BUS: &str = "/run/dbus/system_bus_socket";
const BLUEZ: &str = "org.bluez";
const DEVICE: &str = "org.bluez.Device1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
// Measured in a real scan: 21 signals in 15 s, at most 1176 B each and 3
// per 100 ms (private#770 record, "Buffering proposal"). A full queue stalls
// the connection rather than dropping signals, so size buys slack only.
const MAX_QUEUED: NonZeroUsize = NonZeroUsize::new(64).unwrap();
// Neighbours choose how many devices BlueZ reports; the cache must not grow
// with them.
const MAX_TRACKED_DEVICES: usize = 512;

type Properties = HashMap<String, OwnedVariant>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;

/// BlueZ LE discovery on a bus connection owned by one scan. Dropping the scan
/// closes the socket, and BlueZ releases a discovery session whose client
/// disconnects, so cancellation never waits on a `StopDiscovery` reply that a
/// stalled connection could withhold (private#770).
pub struct BlueZDiscovery {
    bus: PathBuf,
}

impl BlueZDiscovery {
    pub fn system() -> Self {
        Self::at(SYSTEM_BUS)
    }

    pub fn at(bus: impl Into<PathBuf>) -> Self {
        Self { bus: bus.into() }
    }
}

impl Discovery for BlueZDiscovery {
    fn scan(
        &self,
        service_uuid: String,
        events: EventSender,
    ) -> Pin<Box<dyn Future<Output = Result<(), HardwareFailure>> + Send + '_>> {
        Box::pin(scan(&self.bus, service_uuid, events))
    }
}

struct ScanConnection {
    connection: Connection,
    _lease: SocketLease,
}

struct SocketLease(UnixStream);

impl Drop for SocketLease {
    fn drop(&mut self) {
        // zbus streams can keep the connection alive after the scan ends;
        // shutting the shared socket down ends bus ownership regardless.
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

impl ScanConnection {
    async fn open(bus: &Path) -> Result<Self, HardwareFailure> {
        let socket = tokio::net::UnixStream::connect(bus)
            .await
            .and_then(tokio::net::UnixStream::into_std)
            .map_err(|_| HardwareFailure::Unavailable)?;
        let lease = SocketLease(socket.try_clone().map_err(|_| HardwareFailure::Failed)?);
        let connection = zbus::connection::Builder::unix_stream(socket)
            .max_queued(MAX_QUEUED)
            .build()
            .await
            .map_err(|_| HardwareFailure::Unavailable)?;
        Ok(Self {
            connection,
            _lease: lease,
        })
    }
}

async fn scan(
    bus: &Path,
    service_uuid: String,
    mut events: EventSender,
) -> Result<(), HardwareFailure> {
    let service = Uuid::parse_str(&service_uuid).map_err(|_| HardwareFailure::Failed)?;
    let scan = ScanConnection::open(bus).await?;
    let bus = &scan.connection;
    let mut signals = subscribe(bus).await?;
    let objects: Objects = call(bus, "/", OBJECT_MANAGER, "GetManagedObjects", &())
        .await?
        .body()
        .deserialize()
        .map_err(|_| HardwareFailure::Failed)?;
    let adapter = powered_adapter(&objects).ok_or(HardwareFailure::Unavailable)?;
    let mut devices = Devices::default();
    for (path, interfaces) in &objects {
        if let Some(properties) = interfaces.get(DEVICE) {
            devices.update(path, properties);
        }
    }
    let filter = HashMap::from([
        ("UUIDs", zbus::Variant::from(vec![service.to_string()])),
        ("Transport", zbus::Variant::from("le")),
    ]);
    call(
        bus,
        adapter.as_str(),
        "org.bluez.Adapter1",
        "SetDiscoveryFilter",
        &filter,
    )
    .await?;
    call(
        bus,
        adapter.as_str(),
        "org.bluez.Adapter1",
        "StartDiscovery",
        &(),
    )
    .await?;

    while let Some(signal) = signals.next().await {
        let Ok(signal) = signal else { continue };
        let Some(observation) = devices.observe(&signal, &service) else {
            continue;
        };
        match events
            .send(Event::BleAdvertisementObserved(observation))
            .await
        {
            Ok(()) | Err(DeliveryError::PayloadTooLarge | DeliveryError::InvalidEvent) => {}
            Err(DeliveryError::Inactive | DeliveryError::Closed | DeliveryError::Full) => {
                return Ok(());
            }
        }
    }
    Err(HardwareFailure::Failed)
}

/// One rule for every BlueZ signal: separate streams would lose the bus
/// order between a device's `InterfacesAdded` and its `PropertiesChanged`.
async fn subscribe(bus: &Connection) -> Result<MessageStream, HardwareFailure> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(BLUEZ)
        .build()
        .map_err(|_| HardwareFailure::Failed)?;
    MessageStream::for_match_rule(rule, bus, Some(MAX_QUEUED))
        .await
        .map_err(|_| HardwareFailure::Failed)
}

async fn call<B>(
    bus: &Connection,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> Result<Message, HardwareFailure>
where
    B: serde::Serialize + zbus::DynamicType,
{
    bus.call_method(Some(BLUEZ), path, Some(interface), method, body)
        .await
        .map_err(|error| refusal(&error))
}

fn refusal(error: &zbus::Error) -> HardwareFailure {
    let name = match error {
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        zbus::Error::FDO(error) => zbus::DBusError::name(error.as_ref()).to_string(),
        _ => return HardwareFailure::Failed,
    };
    match name.as_str() {
        "org.bluez.Error.NotAuthorized" | "org.freedesktop.DBus.Error.AccessDenied" => {
            HardwareFailure::PermissionDenied
        }
        "org.bluez.Error.NotReady"
        | "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => HardwareFailure::Unavailable,
        _ => HardwareFailure::Failed,
    }
}

fn powered_adapter(objects: &Objects) -> Option<&OwnedObjectPath> {
    let mut adapters: Vec<_> = objects
        .iter()
        .filter_map(|(path, interfaces)| Some((path, interfaces.get("org.bluez.Adapter1")?)))
        .filter(|(_, adapter)| property::<bool>(adapter, "Powered") == Some(true))
        .map(|(path, _)| path)
        .collect();
    adapters.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    adapters.first().copied()
}

fn property<T: TryFrom<OwnedVariant>>(properties: &Properties, name: &str) -> Option<T> {
    T::try_from(properties.get(name)?.try_clone().ok()?).ok()
}

#[derive(Default)]
struct Device {
    address: Option<String>,
    uuids: Vec<Uuid>,
}

#[derive(Default)]
struct Devices(HashMap<OwnedObjectPath, Device>);

impl Devices {
    fn update(&mut self, path: &OwnedObjectPath, properties: &Properties) -> Option<&Device> {
        if !self.0.contains_key(path) && self.0.len() >= MAX_TRACKED_DEVICES {
            return None;
        }
        let device = self.0.entry(path.clone()).or_default();
        if let Some(address) = property::<String>(properties, "Address") {
            device.address = Some(address);
        }
        if let Some(uuids) = property::<Vec<String>>(properties, "UUIDs") {
            device.uuids = uuids
                .iter()
                .filter_map(|u| Uuid::parse_str(u).ok())
                .collect();
        }
        Some(device)
    }

    /// Only a fresh RSSI is an observation: BlueZ also reports devices it
    /// cached from earlier scans.
    fn observe(&mut self, signal: &Message, service: &Uuid) -> Option<BleAdvertisementObservation> {
        let header = signal.header();
        let (path, properties): (OwnedObjectPath, Properties) =
            match (header.interface()?.as_str(), header.member()?.as_str()) {
                (OBJECT_MANAGER, "InterfacesAdded") => {
                    let (path, mut interfaces): (OwnedObjectPath, HashMap<String, Properties>) =
                        signal.body().deserialize().ok()?;
                    (path, interfaces.remove(DEVICE)?)
                }
                (OBJECT_MANAGER, "InterfacesRemoved") => {
                    let (path, _): (OwnedObjectPath, Vec<String>) =
                        signal.body().deserialize().ok()?;
                    self.0.remove(&path);
                    return None;
                }
                (PROPERTIES, "PropertiesChanged") => {
                    let (interface, changed, _): (String, Properties, Vec<String>) =
                        signal.body().deserialize().ok()?;
                    if interface != DEVICE {
                        return None;
                    }
                    (header.path()?.to_owned().into(), changed)
                }
                _ => return None,
            };
        let device = self.update(&path, &properties)?;
        let rssi = property::<i16>(&properties, "RSSI")?;
        if !device.uuids.contains(service) {
            return None;
        }
        let uuids = device
            .uuids
            .iter()
            .map(|uuid| BleServiceUuid::Normalized(*uuid.as_bytes()))
            .collect();
        BleAdvertisementObservation::new(device.address.clone()?, rssi, uuids).ok()
    }
}
