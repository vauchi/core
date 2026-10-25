// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Secrets held on the heap are gone before their memory is freed
//! (ADR-002, vauchi/private#817).
//!
//! A test-only global allocator looks at each block as it is freed, while
//! the memory is still valid, and records whether a marker string is in it.
//! To keep each read defined it zero-fills every allocation, so a byte
//! buffer's spare capacity is never uninitialized, and reads only blocks
//! aligned to 1 (`String` and `Vec<u8>` storage), which cannot hold padding.
//! `test:formal` runs this file under Miri and in release, as
//! `wipe_on_drop_tests.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use vauchi_core::exchange::DeviceLinkResponse;
use vauchi_core::identity::{DeviceRegistry, Identity};

const MARKER: &[u8] = b"vauchi-heap-marker-5a5a5a5a";

static WATCHING: AtomicBool = AtomicBool::new(false);
static MARKER_FREED: AtomicBool = AtomicBool::new(false);
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct FreedBytesInspector;

// SAFETY: every call forwards to `System` with the caller's layout; the
// only addition is reading a block that is still allocated.
unsafe impl GlobalAlloc for FreedBytesInspector {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if WATCHING.load(Ordering::SeqCst) && layout.align() == 1 {
            // SAFETY: the block is still allocated (it is freed below); it
            // started zeroed and holds bytes only, so every byte is
            // initialized.
            let freed = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
            if freed.windows(MARKER.len()).any(|window| window == MARKER) {
                MARKER_FREED.store(true, Ordering::SeqCst);
            }
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: FreedBytesInspector = FreedBytesInspector;

/// Runs `drop_it` and reports whether the marker was in any byte buffer
/// freed meanwhile.
fn marker_freed_while(drop_it: impl FnOnce()) -> bool {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    MARKER_FREED.store(false, Ordering::SeqCst);
    WATCHING.store(true, Ordering::SeqCst);
    drop_it();
    WATCHING.store(false, Ordering::SeqCst);
    MARKER_FREED.load(Ordering::SeqCst)
}

fn payload() -> String {
    format!(
        "{{\"contacts\":[{{\"shared_key\":\"{}\"}}]}}",
        String::from_utf8_lossy(MARKER)
    )
}

/// The instrument itself: a plain `String` is freed with its bytes.
// @internal
#[test]
fn a_plain_string_is_freed_with_its_bytes() {
    let plain = payload();

    assert!(marker_freed_while(move || drop(plain)));
}

// @internal
#[test]
fn a_zeroizing_string_is_wiped_before_it_is_freed() {
    let wiped = zeroize::Zeroizing::new(payload());

    assert!(!marker_freed_while(move || drop(wiped)));
}

/// The sync payload carries every contact's shared key to the new device.
// @internal
#[test]
fn a_device_link_response_wipes_its_sync_payload() {
    let identity = Identity::create("Alice", 0);
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&[0x42; 32]),
        identity.signing_keypair(),
    );
    let response = DeviceLinkResponse::with_sync_payload(
        [0x42; 32],
        "Alice".to_string(),
        1,
        registry,
        payload(),
    );

    assert!(!marker_freed_while(move || drop(response)));
}
