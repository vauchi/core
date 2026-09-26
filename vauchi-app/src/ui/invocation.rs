// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! One-shot invocations for non-interactive shells (ADR-066 Amendment
//! 2026-09-26 (b)).
//!
//! The shell passes a typed [`Invocation`] as construction data; Core
//! applies its arguments, prepares either a text surface or a versioned
//! machine-readable document, and always ends with
//! [`Command::FinishInvocation`]. The shell never reads Core domain state.

use vauchi_core::{
    AccessibilitySpec, AlertSpec, Command, InvocationOutcome, PresentationTokens, SurfaceId,
    SurfaceLayout, SurfaceSpec, api::Vauchi,
};

use crate::i18n::{Locale, get_string};
use crate::theme::DesignTokens;

mod contacts_list;

/// A shell request Core applies without interaction.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Invocation {
    /// List contacts; `limit == 0` means all.
    ContactsList { offset: usize, limit: usize },
    /// List archived contacts.
    ArchivedContactsList,
}

/// What the shell asked to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationOutput {
    /// A prepared surface the shell renders once.
    Text,
    /// A versioned document the shell writes out unread.
    Document,
}

/// Apply `invocation` and return the complete command sequence, always
/// ending in [`Command::FinishInvocation`].
pub fn invoke(
    vauchi: &Vauchi,
    invocation: &Invocation,
    output: InvocationOutput,
    locale: Locale,
) -> Vec<Command> {
    let result = match invocation {
        Invocation::ContactsList { offset, limit } => {
            contacts_list::run(vauchi, *offset, *limit, output, locale)
        }
        Invocation::ArchivedContactsList => contacts_list::run_archived(vauchi, output, locale),
    };
    match result {
        Ok(mut commands) => {
            commands.push(Command::FinishInvocation {
                outcome: InvocationOutcome::Succeeded,
            });
            commands
        }
        Err(_) => vec![
            Command::PresentAlert {
                alert: AlertSpec {
                    title: get_string(locale, "error.generic"),
                    message: get_string(locale, "error.unknown"),
                },
            },
            Command::FinishInvocation {
                outcome: InvocationOutcome::Failed,
            },
        ],
    }
}

fn one_shot_surface(id: &str, title: String, nodes: Vec<vauchi_core::PresentationNode>) -> Command {
    let tokens = DesignTokens::default();
    Command::ReplaceSurface {
        surface: SurfaceSpec {
            surface_id: SurfaceId::new(id).expect("invocation surface ids are valid"),
            revision: 1,
            accessibility_label: title.clone(),
            title,
            subtitle: None,
            layout: SurfaceLayout::Scroll,
            tokens: PresentationTokens {
                spacing_small: tokens.spacing.sm,
                spacing_medium: tokens.spacing.md,
                spacing_large: tokens.spacing.lg,
                corner_radius: tokens.border_radius.md_lg,
                minimum_target_size: tokens.touch_target.minimum,
            },
            nodes,
        },
    }
}

fn accessibility(label: &str) -> AccessibilitySpec {
    AccessibilitySpec::label(label)
}
