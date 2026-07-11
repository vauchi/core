// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-driven Avatar Editor screen (ADR-042).
//!
//! State machine: SourcePicker → Editing/Generating → Complete.
//! Frontends render the screen natively and handle image hardware
//! commands via the command/event protocol (ADR-031).

use vauchi_core::avatar::{generate_initials_avatar, generate_mandelbrot_avatar, normalize_avatar};
use vauchi_core::{Command, Event};

use super::action::{ActionResult, UserAction};
use super::component::{A11y, AccessibilityRole, ActionListItem, Component};
use super::engine::WorkflowEngine;
use super::screen::{ActionStyle, ScreenAction, ScreenModel};
use crate::i18n::{Locale, get_string};

/// Avatar generation style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerateStyle {
    Initials,
    Mandelbrot,
}

/// Internal state of the avatar editor.
#[derive(Clone, Debug)]
enum State {
    /// Pick a source: camera, photo library, file, or generate.
    SourcePicker,
    /// Image selected — show crop preview + brightness slider.
    Editing {
        image_data: Vec<u8>,
        brightness: f32,
    },
    /// Generator — pick style and preview generated avatar.
    Generating {
        style: GenerateStyle,
        bg_color: [u8; 3],
        mandelbrot_seed: u64,
        preview_data: Vec<u8>,
    },
}

/// Predefined color palette for initials avatars.
const INITIALS_COLORS: [[u8; 3]; 8] = [
    [59, 130, 246], // blue
    [16, 185, 129], // green
    [249, 115, 22], // orange
    [139, 92, 246], // purple
    [239, 68, 68],  // red
    [236, 72, 153], // pink
    [20, 184, 166], // teal
    [99, 102, 241], // indigo
];

/// Core-driven avatar editor engine.
pub struct AvatarEditorEngine {
    state: State,
    display_name: String,
    result: Option<Vec<u8>>,
    cancelled: bool,
    has_existing_avatar: bool,
    /// Set when user chose "Remove" — distinct from cancel (no avatar = intentional clear).
    removed: bool,
    locale: Locale,
}

impl AvatarEditorEngine {
    pub fn new(display_name: String, has_existing_avatar: bool) -> Self {
        Self {
            state: State::SourcePicker,
            display_name,
            has_existing_avatar,
            result: None,
            cancelled: false,
            removed: false,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-6).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns `true` if the user chose to remove the existing avatar.
    pub fn avatar_removed(&self) -> bool {
        self.removed
    }

    /// Read the resulting avatar after `Complete`. Returns `None` if
    /// cancelled or no avatar was produced.
    pub fn result_avatar(&self) -> Option<&[u8]> {
        self.result.as_deref()
    }

    fn initials(&self) -> String {
        let parts: Vec<&str> = self.display_name.split_whitespace().collect();
        let first = parts.first().and_then(|s| s.chars().next()).unwrap_or('?');
        let last = if parts.len() > 1 {
            parts.last().and_then(|s| s.chars().next())
        } else {
            None
        };
        match last {
            Some(l) => format!("{}{}", first.to_uppercase(), l.to_uppercase()),
            None => first.to_uppercase().to_string(),
        }
    }

    /// Dispatch a tap on the avatar source picker (the `"sources"`
    /// `ActionList`). Returns `Some(result)` for a known source id,
    /// `None` for an unrecognised id (caller falls through to
    /// `UpdateScreen`).
    ///
    /// Renderers emit `UserAction::ListItemSelected` for `ActionList`
    /// rows; the legacy `ActionPressed` arm routes here too so both
    /// dispatch shapes resolve to the same logic.
    fn handle_source_selection(&mut self, source_id: &str) -> Option<ActionResult> {
        match source_id {
            "remove_avatar" => {
                self.removed = true;
                Some(ActionResult::Complete)
            }
            "source_camera" => Some(ActionResult::Commands {
                commands: vec![Command::ImageCaptureFromCamera],
            }),
            "source_photos" => Some(ActionResult::Commands {
                commands: vec![Command::ImagePickFromLibrary, Command::ImagePickFromFile],
            }),
            "source_generate" => {
                self.enter_generating(GenerateStyle::Initials);
                Some(ActionResult::UpdateScreen(self.current_screen()))
            }
            _ => None,
        }
    }

    fn build_source_picker(&self) -> ScreenModel {
        let mut items = vec![
            ActionListItem {
                id: "source_camera".into(),
                label: self.t("exchange.requirement.camera"),
                icon: Some("camera".into()),
                detail: None,
                a11y: Some(A11y::labeled(self.t("exchange.requirement.camera"))),
                info_key: None,
            },
            ActionListItem {
                id: "source_photos".into(),
                label: self.t("avatar.source_photos"),
                icon: Some("photo".into()),
                detail: None,
                a11y: Some(A11y::labeled(self.t("avatar.source_photos"))),
                info_key: None,
            },
            ActionListItem {
                id: "source_generate".into(),
                label: self.t("avatar.source_generate"),
                icon: Some("sparkles".into()),
                detail: None,
                a11y: Some(A11y::labeled(self.t("avatar.source_generate"))),
                info_key: None,
            },
        ];
        if self.has_existing_avatar {
            items.push(ActionListItem {
                id: "remove_avatar".into(),
                label: self.t("avatar.remove"),
                icon: Some("trash".into()),
                detail: None,
                a11y: Some(A11y::labeled(self.t("avatar.remove"))),
                info_key: None,
            });
        }
        ScreenModel::new(
            "avatar_editor",
            self.t("avatar.choose_title"),
            vec![Component::ActionList {
                id: "sources".into(),
                items,
            }],
            vec![ScreenAction {
                id: "cancel".into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
        )
    }

    fn build_editing_screen(&self, image_data: &[u8], brightness: f32) -> ScreenModel {
        ScreenModel::new(
            "avatar_editor",
            self.t("avatar.edit_title"),
            vec![
                Component::AvatarPreview {
                    id: "preview".into(),
                    image_data: Some(image_data.to_vec()),
                    initials: self.initials(),
                    bg_color: None,
                    brightness,
                    editable: false,
                    edit_action_id: None,
                    a11y: Some(A11y {
                        label: Some(self.t("avatar.preview_a11y")),
                        hint: None,
                        role: Some(AccessibilityRole::Image),
                    }),
                },
                Component::Slider {
                    id: "brightness".into(),
                    label: self.t("avatar.brightness_label"),
                    value: brightness,
                    min: -0.3,
                    max: 0.3,
                    step: 0.0,
                    min_icon: Some("sun.min".into()),
                    max_icon: Some("sun.max".into()),
                    a11y: Some(A11y::labeled(self.t("avatar.brightness_label"))),
                },
            ],
            vec![
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
                ScreenAction {
                    id: "save".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
            ],
        )
    }

    fn build_generating_screen(
        &self,
        style: &GenerateStyle,
        bg_color: &[u8; 3],
        preview_data: &[u8],
    ) -> ScreenModel {
        let mut components = vec![Component::AvatarPreview {
            id: "preview".into(),
            image_data: Some(preview_data.to_vec()),
            initials: self.initials(),
            bg_color: Some(*bg_color),
            brightness: 0.0,
            editable: false,
            edit_action_id: None,
            a11y: Some(A11y {
                label: Some(self.t("avatar.generated_preview_a11y")),
                hint: None,
                role: Some(AccessibilityRole::Image),
            }),
        }];

        // Style picker
        components.push(Component::ActionList {
            id: "gen_style".into(),
            items: vec![
                ActionListItem {
                    id: "initials".into(),
                    label: self.t("avatar.style_initials"),
                    icon: Some("textformat".into()),
                    detail: None,
                    a11y: Some(A11y::labeled(self.t("avatar.style_initials"))),
                    info_key: None,
                },
                ActionListItem {
                    id: "mandelbrot".into(),
                    label: self.t("avatar.style_mandelbrot"),
                    icon: Some("sparkles".into()),
                    detail: None,
                    a11y: Some(A11y::labeled(self.t("avatar.style_mandelbrot"))),
                    info_key: None,
                },
            ],
        });

        // Color palette (initials only)
        if *style == GenerateStyle::Initials {
            let color_items = INITIALS_COLORS
                .iter()
                .enumerate()
                .map(|(i, c)| ActionListItem {
                    id: format!("color_{i}"),
                    label: String::new(),
                    icon: None,
                    detail: Some(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])),
                    a11y: None,
                    info_key: None,
                })
                .collect();
            components.push(Component::ActionList {
                id: "colors".into(),
                items: color_items,
            });
        }

        let mut actions = vec![
            ScreenAction {
                id: "cancel".into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            },
            ScreenAction {
                id: "use".into(),
                label: self.t("avatar.use_button"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("avatar.use_button"))),
            },
        ];

        if *style == GenerateStyle::Mandelbrot {
            actions.push(ScreenAction {
                id: "regenerate".into(),
                label: self.t("avatar.regenerate_button"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("avatar.regenerate_button"))),
            });
        }

        ScreenModel::new(
            "avatar_editor",
            self.t("avatar.generate_title"),
            components,
            actions,
        )
    }

    fn enter_generating(&mut self, style: GenerateStyle) {
        let bg_color = INITIALS_COLORS[0];
        let preview_data = match style {
            GenerateStyle::Initials => generate_initials_avatar(bg_color, 256),
            GenerateStyle::Mandelbrot => generate_mandelbrot_avatar(0, 256),
        };
        self.state = State::Generating {
            style,
            bg_color,
            mandelbrot_seed: 0,
            preview_data,
        };
    }
}

impl WorkflowEngine for AvatarEditorEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::AvatarEditor {
            removed: self.avatar_removed(),
            avatar: self.result_avatar().map(|a| a.to_vec()),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        match &self.state {
            State::SourcePicker => self.build_source_picker(),
            State::Editing {
                image_data,
                brightness,
            } => self.build_editing_screen(image_data, *brightness),
            State::Generating {
                style,
                bg_color,
                preview_data,
                ..
            } => self.build_generating_screen(style, bg_color, preview_data),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // ── Cancel (any state) ──────────────────────────────
            UserAction::ActionPressed { action_id } if action_id == "cancel" => {
                self.cancelled = true;
                ActionResult::Complete
            }

            // ── Source picker rows ──────────────────────────────
            // The four source options live in `ActionList { id: "sources" }`.
            // Renderers emit `ListItemSelected` for ActionList rows; the
            // `ActionPressed` arm below is a thin fallback so both shapes
            // route to the same per-source logic.
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "sources" => self
                .handle_source_selection(&item_id)
                .unwrap_or_else(|| ActionResult::UpdateScreen(self.current_screen())),
            UserAction::ActionPressed { action_id }
                if matches!(
                    action_id.as_str(),
                    "remove_avatar" | "source_camera" | "source_photos" | "source_generate"
                ) =>
            {
                self.handle_source_selection(&action_id)
                    .unwrap_or_else(|| ActionResult::UpdateScreen(self.current_screen()))
            }

            // ── Editing actions ─────────────────────────────────
            UserAction::ActionPressed { action_id } if action_id == "save" => {
                if let State::Editing { ref image_data, .. } = self.state {
                    // Normalize to WebP (brightness is a display-only effect
                    // — we store the original normalized image)
                    match normalize_avatar(image_data) {
                        Ok(webp) => {
                            self.result = Some(webp);
                            ActionResult::Complete
                        }
                        Err(_) => ActionResult::ShowAlert {
                            title: self.t("status.error"),
                            message: self.t("avatar.process_error"),
                        },
                    }
                } else {
                    ActionResult::UpdateScreen(self.current_screen())
                }
            }
            UserAction::SliderChanged {
                component_id,
                value_milli,
            } if component_id == "brightness" => {
                if let State::Editing {
                    ref mut brightness, ..
                } = self.state
                {
                    *brightness = (value_milli as f32 / 1000.0).clamp(-0.3, 0.3);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }

            // ── Generator actions ───────────────────────────────
            UserAction::ActionPressed { action_id } if action_id == "use" => {
                if let State::Generating {
                    ref preview_data, ..
                } = self.state
                {
                    self.result = Some(preview_data.clone());
                    ActionResult::Complete
                } else {
                    ActionResult::UpdateScreen(self.current_screen())
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "regenerate" => {
                if let State::Generating {
                    ref mut mandelbrot_seed,
                    ref mut preview_data,
                    style: GenerateStyle::Mandelbrot,
                    ..
                } = self.state
                {
                    *mandelbrot_seed = mandelbrot_seed.wrapping_add(1);
                    *preview_data = generate_mandelbrot_avatar(*mandelbrot_seed, 256);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "gen_style" => {
                let new_style = match item_id.as_str() {
                    "mandelbrot" => GenerateStyle::Mandelbrot,
                    _ => GenerateStyle::Initials,
                };
                self.enter_generating(new_style);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "colors" => {
                if let Some(idx) = item_id.strip_prefix("color_")
                    && let Ok(i) = idx.parse::<usize>()
                    && let Some(&color) = INITIALS_COLORS.get(i)
                    && let State::Generating {
                        ref mut bg_color,
                        ref mut preview_data,
                        style: GenerateStyle::Initials,
                        ..
                    } = self.state
                {
                    *bg_color = color;
                    *preview_data = generate_initials_avatar(color, 256);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }

            // ── Fallback ────────────────────────────────────────
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        match event {
            Event::ImageReceived { data } => {
                match normalize_avatar(&data) {
                    Ok(webp) => {
                        self.state = State::Editing {
                            image_data: webp,
                            brightness: 0.0,
                        };
                    }
                    Err(_) => {
                        // Invalid image — stay on source picker, show alert
                        return Some(ActionResult::ShowAlert {
                            title: self.t("avatar.invalid_image_title"),
                            message: self.t("avatar.invalid_image_message"),
                        });
                    }
                }
                Some(ActionResult::UpdateScreen(self.current_screen()))
            }
            Event::ImagePickCancelled => {
                // Return to source picker (or stay if already there)
                self.state = State::SourcePicker;
                Some(ActionResult::UpdateScreen(self.current_screen()))
            }
            Event::PermissionDenied { ref transport } if transport == "camera" => {
                Some(ActionResult::ShowAlert {
                    title: self.t("avatar.camera_access_title"),
                    message: self.t("avatar.camera_permission_denied"),
                })
            }
            Event::HardwareUnavailable { .. } => {
                // Platform doesn't support this — ignore silently
                None
            }
            _ => None,
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}
