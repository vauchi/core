// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-side socket for local device linking (ADR-070 Phase 1).
//!
//! Bound when the QR is shown and dropped when the ceremony ends or its
//! window expires, so the socket exists only for as long as a scanned QR
//! could still be acted on. There is no idle listener between ceremonies.
//!
//! **Protocol**: connect, write one frame, half-close, read the response.
//! The half-close is what bounds the read — without it the host cannot tell
//! a finished request from a peer that stopped mid-frame. See
//! [`super::local_wire`] for the frame format and its limits.
//!
//! Connections are served one at a time. A peer that connects and says
//! nothing therefore costs `read_timeout`, not the ceremony: the timeout is
//! what stops a silent peer from wedging the accept loop, and that is
//! asserted rather than assumed.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use vauchi_core::monotonic::MonotonicClock;
use vauchi_core::rng::SecureRng;
use vauchi_core::sleeper::Sleeper;

use super::local_rendezvous::SingleCeremonyRendezvous;
use super::local_wire::{MAX_FRAME_BYTES, decode_request, encode_response, serve};

/// Bytes of entropy behind a minted rendezvous code.
///
/// The relay issues six digits and backs them with per-code and global
/// claim rate limits (`device_link_responder_machine.rs`). A local host has
/// no limiter, so the code itself has to be unguessable: 128 bits, hex
/// encoded, well inside `local_wire::MAX_CODE_BYTES`.
const CODE_ENTROPY_BYTES: usize = 16;

/// How long the accept loop waits between polls when no peer is waiting.
/// Short enough that stopping is prompt, long enough not to spin.
const ACCEPT_POLL: Duration = Duration::from_millis(20);

/// Mint a rendezvous code with full CSPRNG entropy.
pub fn mint_code(rng: &dyn SecureRng) -> String {
    let mut bytes = [0u8; CODE_ENTROPY_BYTES];
    rng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// The injected seams the accept loop runs on.
///
/// Reading the clock directly, or blocking the thread directly, is barred
/// here — the pure-functional-core ratchet enforces it — so both arrive as
/// traits instead. A test can therefore drive the window without waiting on
/// real time, which is why the machines take `now` explicitly too.
pub struct ListenerRuntime {
    /// Mints rendezvous codes; must be a CSPRNG (see [`mint_code`]).
    pub rng: Arc<dyn SecureRng>,
    /// Bounds the ceremony window. Monotonic, so a wall-clock jump cannot
    /// extend or collapse a live ceremony.
    pub clock: Arc<dyn MonotonicClock>,
    /// Paces the accept loop between polls.
    pub sleeper: Arc<dyn Sleeper>,
}

/// A bound rendezvous socket. Dropping it stops the listener.
pub struct LocalRendezvousListener {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalRendezvousListener {
    /// Bind on the loopback-reachable interface and serve `rendezvous`
    /// until `window` elapses, or until this value is dropped.
    ///
    /// Binds to port 0: the OS assigns the port and [`addr`](Self::addr)
    /// reports it, so nothing well-known is occupied and two ceremonies can
    /// never collide on a port.
    pub fn bind(
        rendezvous: Arc<SingleCeremonyRendezvous>,
        runtime: ListenerRuntime,
        window: Duration,
        read_timeout: Duration,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let ListenerRuntime {
            rng,
            clock,
            sleeper,
        } = runtime;
        let deadline = clock.now() + window;

        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) && clock.now() < deadline {
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        serve_connection(stream, &rendezvous, rng.as_ref(), read_timeout);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        sleeper.sleep(ACCEPT_POLL);
                    }
                    // A failed accept says nothing about the next one, and
                    // giving up would strand a ceremony the user can still
                    // see on screen.
                    Err(_) => sleeper.sleep(ACCEPT_POLL),
                }
            }
        });

        Ok(Self {
            addr,
            stop,
            worker: Some(worker),
        })
    }

    /// The address a joiner should be pointed at.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for LocalRendezvousListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            // The loop checks `stop` at most one `ACCEPT_POLL` away, so this
            // joins promptly rather than waiting out the ceremony window.
            drop(worker.join());
        }
    }
}

/// Read one frame, answer it, and close.
///
/// Every failure closes the connection without a reply. A peer that sent
/// something unreadable learns only that, which is the same thing every
/// other refusal tells it (see `local_wire`'s coarse refusals).
fn serve_connection(
    mut stream: TcpStream,
    rendezvous: &SingleCeremonyRendezvous,
    rng: &dyn SecureRng,
    read_timeout: Duration,
) {
    // Accepted sockets inherit the listener's non-blocking flag on
    // macOS/BSD but not on Linux. Left as-is, a read returns `WouldBlock`
    // the instant no byte is buffered, so a peer whose frame arrives split
    // across packets is dropped without a reply — intermittently, and only
    // on some platforms. Force blocking so `read_timeout` is what bounds
    // the read everywhere.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(read_timeout)).is_err()
    {
        return;
    }

    // One byte past the limit, so an oversized frame is still *detected* by
    // the decoder rather than silently truncated into a valid-looking one.
    let mut frame = Vec::new();
    let capped = (MAX_FRAME_BYTES + 1) as u64;
    if Read::by_ref(&mut stream)
        .take(capped)
        .read_to_end(&mut frame)
        .is_err()
    {
        return;
    }

    let Ok(request) = decode_request(&frame) else {
        return;
    };

    let response = serve(rendezvous, request, &mint_code(rng));
    drop(stream.write_all(&encode_response(&response)));
    drop(stream.flush());
}
