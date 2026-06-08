// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal in-process HTTP mock relay for `HttpTransport` tests.
//!
//! Speaks just enough HTTP/1.1 to drive the V2 JSON request/response
//! protocol used by `core/vauchi-core/src/network/http_transport.rs`.
//! No external HTTP server crate is pulled in — a single
//! `std::net::TcpListener` thread parses requests, matches them
//! against a programmed response table, and returns canned responses.
//!
//! Lifecycle:
//! 1. `MockRelay::start()` — starts the listener thread, returns a
//!    handle whose `.url()` method gives the base URL to feed into
//!    `HttpTransportConfig::for_testing(...)`.
//! 2. Tests call `queue(action, response)` (and friends) to program
//!    the responses for upcoming POSTs to `/v2/<action>`.
//! 3. After running the unit under test, `mock.received(action)` lets
//!    you assert what the client actually sent (URL path + raw body).
//! 4. Drop closes the listener, joins the thread.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One programmed response for a single `/v2/<action>` POST.
#[derive(Clone, Debug)]
pub struct CannedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Response body. For 2xx responses with a JSON content-type this is
    /// typically a serialised `V2Response`; for 426/429 it is irrelevant.
    pub body: Vec<u8>,
}

impl CannedResponse {
    /// Convenience constructor for a 200 OK response with a JSON body.
    pub fn ok_json(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into(),
        }
    }

    /// 200 OK with a serde-serialised body.
    pub fn ok_v2_response(resp: &vauchi_protocol::v2::V2Response) -> Self {
        Self::ok_json(serde_json::to_vec(resp).expect("V2Response serializes"))
    }

    /// HTTP 429 Too Many Requests with optional `Retry-After` header.
    pub fn rate_limited(retry_after_secs: Option<u64>) -> Self {
        let mut headers = vec![("Content-Type".into(), "application/json".into())];
        if let Some(s) = retry_after_secs {
            headers.push(("Retry-After".into(), s.to_string()));
        }
        Self {
            status: 429,
            headers,
            body: br#"{"status":"error"}"#.to_vec(),
        }
    }

    /// HTTP 426 Upgrade Required with `X-Min-Version` header.
    pub fn upgrade_required(min_version: u16) -> Self {
        Self {
            status: 426,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("X-Min-Version".into(), min_version.to_string()),
            ],
            body: br#"{"status":"error"}"#.to_vec(),
        }
    }

    /// Arbitrary status code with empty body.
    pub fn status(code: u16) -> Self {
        Self {
            status: code,
            headers: vec![],
            body: vec![],
        }
    }

    /// Add a custom header to this response.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Captured request — what the client actually sent.
#[derive(Clone, Debug)]
pub struct ReceivedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Shared state between the listener thread and test thread.
#[derive(Default)]
struct State {
    /// Per-action response queues. Path key is `"/v2/<action>"`.
    queues: std::collections::HashMap<String, VecDeque<CannedResponse>>,
    /// Captured requests in arrival order.
    received: Vec<ReceivedRequest>,
    /// Default response when no queue entry matches.
    default: Option<CannedResponse>,
}

pub struct MockRelay {
    addr: std::net::SocketAddr,
    state: Arc<Mutex<State>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockRelay {
    /// Bind to 127.0.0.1 on a free port and start the listener thread.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local_addr");
        let state = Arc::new(Mutex::new(State::default()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let state_for_thread = state.clone();
        let shutdown_for_thread = shutdown.clone();
        let handle = std::thread::Builder::new()
            .name("mock-relay".into())
            .spawn(move || run_listener(listener, state_for_thread, shutdown_for_thread))
            .expect("spawn listener");

        Self {
            addr,
            state,
            shutdown,
            handle: Some(handle),
        }
    }

    /// Base URL the client should POST to (e.g. `http://127.0.0.1:54321`).
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Queue a response for the next POST to `/v2/<action>`.
    pub fn queue(&self, action: &str, response: CannedResponse) {
        let mut s = self.state.lock().unwrap();
        s.queues
            .entry(format!("/v2/{action}"))
            .or_default()
            .push_back(response);
    }

    /// Set the fallback response for any path with no specific queue.
    pub fn set_default(&self, response: CannedResponse) {
        self.state.lock().unwrap().default = Some(response);
    }

    /// Snapshot of every received request in arrival order.
    pub fn received(&self) -> Vec<ReceivedRequest> {
        self.state.lock().unwrap().received.clone()
    }

    /// Convenience: the last received request, panicking if there's none.
    pub fn last_received(&self) -> ReceivedRequest {
        self.received()
            .pop()
            .expect("at least one request must have been received")
    }
}

impl Drop for MockRelay {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Poke the listener with a connection to unblock accept().
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_listener(listener: TcpListener, state: Arc<Mutex<State>>, shutdown: Arc<AtomicBool>) {
    // Blocking accept(): wakes immediately on a real connection regardless of
    // scheduler pressure. The previous non-blocking + 5 ms-poll spin could
    // starve this thread past the client's 2 s timeout under heavy parallel
    // load (full nextest run), producing a spurious request timeout — see
    // `_private/docs/problems/2026-05-25-mock-relay-flake-under-parallelism`.
    // Teardown stays clean: `Drop` sets `shutdown` then self-connects to
    // unblock the pending accept, which we detect via the flag and break.
    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                // Each request is short-lived — handle inline. ureq sends
                // requests sequentially in our tests, so single-threaded
                // handling is sufficient.
                let _ = handle_connection(stream, state.clone());
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(2000)))?;
    stream.set_write_timeout(Some(Duration::from_millis(2000)))?;

    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut headers = Vec::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    // Find a matching response (and capture the request) under one lock.
    let response = {
        let mut s = state.lock().unwrap();
        s.received.push(ReceivedRequest {
            method: method.clone(),
            path: path.clone(),
            headers: headers.clone(),
            body: body.clone(),
        });

        let queue_response = s.queues.get_mut(&path).and_then(|q| q.pop_front());
        queue_response
            .or_else(|| s.default.clone())
            .unwrap_or_else(|| CannedResponse::status(500))
    };

    write_response(&mut stream, &response)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn write_response(stream: &mut TcpStream, response: &CannedResponse) -> std::io::Result<()> {
    let status_text = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };

    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        status_text,
        response.body.len(),
    );
    let mut has_content_type = false;
    for (name, value) in &response.headers {
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !has_content_type && !response.body.is_empty() {
        head.push_str("Content-Type: application/octet-stream\r\n");
    }
    head.push_str("\r\n");

    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}
