// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The render context a shell pushes, and the pick Core hands the shell to
//! keep (vauchi/private#790, #814).

use super::{AppEngine, AppScreen};

impl AppEngine {
    /// Returns the active render context (locale + theme_id) pushed
    /// by the frontend. Used by `screens.rs` to render Settings
    /// dropdown `selected` values without pulling state from the
    /// vault. See [ADR-047](../../../../_private/docs/decisions/adr-047-settings-storage-by-sensitivity.md).
    pub fn render_context(&self) -> &crate::ui::RenderContext {
        &self.render_context
    }

    /// Replace the active render context. Called from PAE
    /// `set_render_context_json` after JSON deserialization;
    /// frontends invoke this at boot and on every Settings
    /// locale/theme dropdown change. Invalidates the Settings
    /// screen cache AND — when the active screen is Settings —
    /// rebuilds the active engine so the next `current_screen()`
    /// call reflects the new dropdown `selected` value without
    /// requiring a navigate-away-and-back round trip.
    ///
    /// A change of rendered language invalidates every cached screen,
    /// because the shell's first push can arrive after the start screen
    /// was built in English (vauchi/private#721).
    ///
    /// A value the shell keeps for Core carries the pick and makes the
    /// vault copy redundant; without one, the vault copy applies and is
    /// handed to the shell to keep (vauchi/private#814).
    pub fn set_render_context(&mut self, mut ctx: crate::ui::RenderContext) {
        if ctx.restorable_state.is_none() {
            ctx.restorable_state = self.kept_state_file.as_ref().and_then(|file| file.read());
        }
        if ctx.apply_restorable_state() {
            self.drop_vault_appearance();
        } else if let Some(pick) = ctx.apply_saved_appearance(&self.vauchi) {
            self.keep_restorable_state(&pick);
        }
        ctx.system_color_scheme = ctx
            .system_color_scheme
            .or(self.render_context.system_color_scheme);
        self.apply_render_context(ctx);
    }

    /// Keeps the language and theme pick in `data_dir`, outside the vault,
    /// so no shell stores it (vauchi/private#814).
    pub fn keep_restorable_state_in(&mut self, data_dir: &std::path::Path) {
        self.kept_state_file = Some(crate::ui::restorable_state::KeptStateFile::in_dir(data_dir));
        let ctx = crate::ui::RenderContext {
            restorable_state: None,
            ..self.render_context.clone()
        };
        self.set_render_context(ctx);
    }

    fn drop_vault_appearance(&self) {
        let Ok(mut flags) = self.vauchi.load_settings_flags() else {
            return;
        };
        if flags.appearance.take().is_some() {
            // Best effort: a copy left behind is only read when the shell
            // hands nothing back, and then it is the pick anyway.
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.save_settings_flags(&flags);
        }
    }

    /// Keeps the pick in the data directory when Core has one; otherwise
    /// queues it for a shell that keeps it, replacing one still queued.
    pub(crate) fn keep_restorable_state(&mut self, pick: &vauchi_core::settings::AppearancePick) {
        if let Some(file) = &self.kept_state_file {
            // Best effort: until the file is written the vault copy still
            // carries the pick.
            #[allow(clippy::let_underscore_must_use)]
            let _ = file.write(&crate::ui::restorable_state::encode(pick));
            return;
        }
        self.pending_commands
            .retain(|command| !matches!(command, vauchi_core::Command::SaveRestorableState { .. }));
        self.pending_commands
            .push_back(vauchi_core::Command::SaveRestorableState {
                state: crate::ui::restorable_state::encode(pick),
            });
    }

    pub(crate) fn apply_render_context(&mut self, ctx: crate::ui::RenderContext) {
        let language_changed = ctx.resolved_locale() != self.render_context.resolved_locale();
        self.wanted_locales_changed |=
            ctx.wanted_locale_codes() != self.render_context.wanted_locale_codes();
        self.render_context = ctx;
        self.refresh_content_snapshot();
        if language_changed {
            self.engine_cache.clear();
        } else {
            self.engine_cache.remove(&AppScreen::Settings);
            self.engine_cache.remove(&AppScreen::SettingsAppearance);
        }
        if language_changed
            || matches!(
                self.screen,
                AppScreen::Settings | AppScreen::SettingsAppearance
            )
        {
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            );
        }
    }
}
