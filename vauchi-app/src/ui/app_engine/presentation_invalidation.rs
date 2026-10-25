// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::{Mutex, mpsc};

use super::AppEngine;

#[derive(Default)]
pub(super) struct PresentationInvalidation {
    sender: Mutex<Option<mpsc::SyncSender<()>>>,
}

impl PresentationInvalidation {
    pub(super) fn notify(&self) {
        let mut sender = self
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(active) = sender.as_ref()
            && matches!(
                active.try_send(()),
                Err(mpsc::TrySendError::Disconnected(()))
            )
        {
            *sender = None;
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<()> {
        let (sender, receiver) = mpsc::sync_channel(1);
        // A replacement must refresh even if the old receiver held the wakeup.
        sender.try_send(()).expect("new invalidation slot is empty");
        *self
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(sender);
        receiver
    }
}

impl AppEngine {
    /// Subscribe to opaque presentation invalidations. Request
    /// [`Self::initial_commands`] on the owning event loop to repaint the
    /// current presentation without discarding in-progress editor state.
    ///
    /// At most one wakeup is queued. Registration includes an initial wakeup
    /// and replaces the previous sender; discard any previous receiver.
    /// Dropping the engine disconnects the receiver. No shell callback runs
    /// inside Core or while the engine is locked.
    pub fn subscribe_presentation_invalidations(&self) -> mpsc::Receiver<()> {
        self.presentation_invalidation.subscribe()
    }
}
