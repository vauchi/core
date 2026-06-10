// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Domain-grouped arm bodies of
//! [`crate::platform_app_engine::PlatformAppEngine::dispatch_domain_command`].
//! One module per command family; each hosts a `dispatch_<family>`
//! method the single exported entry point delegates to.

mod contacts;
mod delivery;
mod devices;
mod engagement;
mod groups_visibility;
mod own_card_identity;
mod recovery_backup;
mod security;
