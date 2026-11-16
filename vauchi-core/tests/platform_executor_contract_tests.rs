// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

mod executor_conformance {
    use std::num::NonZeroUsize;
    use vauchi_core::platform::executor::{DeliveryError, EventInbox, InboxLimits, RejectedEvent};
    use vauchi_core::{BleLinkDirection, Event};

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shared/executor_conformance.rs"
    ));
}
