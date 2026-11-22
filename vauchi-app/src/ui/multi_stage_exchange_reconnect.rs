// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The screens of a repeat exchange where one phone still holds the
//! contact and the other holds nothing (vauchi/private#1285): the
//! question on the phone that holds it, the wait on the other, and the
//! endings that save nothing new. A child module of
//! `multi_stage_exchange.rs` for its size (VRS04); loaded via `#[path]`.
//!
//! The question and the wait keep the code on screen: it has to stay
//! where the peer reads it while a person decides.

use super::*;

/// "Start a new connection": the person agrees to give the contact they
/// hold this exchange's key.
pub const RECONNECT_ACCEPT_ACTION_ID: &str = "reconnect_accept";
/// "Leave it": the contact keeps the connection it had.
pub const RECONNECT_DECLINE_ACTION_ID: &str = "reconnect_decline";

const COMPONENT_ID_QUESTION: &str = "reconnect_question";
const COMPONENT_ID_WAITING: &str = "reconnect_waiting";

/// Whether `screen` asks the reconnect question: the one exchange screen
/// that scrolls, for its text, while it shows a code the peer reads.
#[cfg(feature = "network-rustls")]
pub(crate) fn asks_to_reconnect(screen: &ScreenModel) -> bool {
    screen.components.iter().any(
        |component| matches!(component, Component::Text { id, .. } if id == COMPONENT_ID_QUESTION),
    )
}

/// The 65 % layout in the top row (5, 6, 7 from the left) in the column
/// `layout` draws its code in; the middle for a code that fills the square.
fn top_row_layout(layout: u8) -> u8 {
    const TOP_LEFT: u8 = 5;
    const TOP_MIDDLE: u8 = 6;
    const TOP_RIGHT: u8 = 7;
    match layout {
        1..=4 if layout % 2 == 1 => TOP_LEFT,
        1..=4 => TOP_RIGHT,
        5..=13 => TOP_LEFT + (layout - TOP_LEFT) % 3,
        _ => TOP_MIDDLE,
    }
}

/// Where a repeat exchange stands when the two phones hold different
/// things for each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatExchange {
    /// This phone holds the peer: its person is asked.
    Asking,
    /// This phone holds nothing: the peer's person is asked.
    Waiting,
    /// It ended without a new connection.
    Ended(RepeatEnding),
}

/// Why a repeat exchange ended without a new connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatEnding {
    /// This phone's person declined.
    KeptByYou,
    /// The other phone's person declined, or that phone kept its
    /// connection without asking.
    KeptByThem,
    /// Nobody answered before the deadline.
    TimedOut,
}

impl RepeatEnding {
    fn component_id(self) -> &'static str {
        match self {
            RepeatEnding::KeptByYou => "reconnect_kept_by_you",
            RepeatEnding::KeptByThem => "reconnect_kept_by_them",
            RepeatEnding::TimedOut => "reconnect_timed_out",
        }
    }

    fn text_key(self) -> &'static str {
        match self {
            RepeatEnding::KeptByYou => "exchange.reconnect.kept_by_you",
            RepeatEnding::KeptByThem => "exchange.reconnect.kept_by_them",
            RepeatEnding::TimedOut => "exchange.reconnect.timed_out",
        }
    }
}

impl MultiStageExchangeEngine {
    /// Set by the AppEngine when a finalized exchange has to ask or wait,
    /// and cleared once it ended in a save.
    pub fn set_repeat(&mut self, repeat: Option<RepeatExchange>) {
        if self.cancelled {
            return;
        }
        self.repeat = repeat;
    }

    /// The person's answer to the question, once given. Latched: the
    /// AppEngine acts on it once, and a later tap cannot change it.
    pub fn reconnect_answer(&self) -> Option<bool> {
        self.reconnect_answer
    }

    /// Where the own code is drawn in its square. While the question
    /// waits for its answer the two answers are kept in view under the
    /// code and an iPhone SE shows about 86 % of the square: a code the
    /// trainer put in the lower row was cut there and the other phone
    /// decoded none of it (rig, 2026-11-22, vauchi/private#1304). The top
    /// row at 65 % ends at 70 % of the square. Once answered, the other
    /// phone reads this one again at the layout it was trained to.
    pub(super) fn own_code_placement(&self) -> Option<QrPlacement> {
        let asking = self.repeat == Some(RepeatExchange::Asking) && self.reconnect_answer.is_none();
        let layout = if asking {
            top_row_layout(self.current_qr_layout)
        } else {
            self.current_qr_layout
        };
        super::placement_for_layout(layout)
    }

    pub(super) fn answer_reconnect(&mut self, accept: bool) -> ActionResult {
        if self.repeat == Some(RepeatExchange::Asking) && self.reconnect_answer.is_none() {
            self.reconnect_answer = Some(accept);
        }
        ActionResult::UpdateScreen(self.build_screen())
    }

    fn t_named(&self, key: &str) -> String {
        match &self.peer_name {
            Some(name) => get_string_with_args(self.locale, key, &[("name", name)]),
            None => self.t(key),
        }
    }

    fn repeat_status(&self, id: &str, label: String, detail: String) -> Component {
        Component::Text {
            id: id.into(),
            content: format!("{label} · {detail}"),
            style: TextStyle::Body,
            a11y: Some(A11y {
                label: Some(label),
                hint: Some(detail),
                role: None,
            }),
        }
    }

    pub(super) fn build_repeat_screen(&self, title: String, repeat: RepeatExchange) -> ScreenModel {
        match repeat {
            RepeatExchange::Asking => {
                // One line, over the code: with the two answers kept in
                // view under it, that is what an iPhone SE has room for
                // with the code whole. Under the code the question was off
                // the screen, and beside the camera it was cut to one line
                // of three (rig, 2026-11-19 to 21, vauchi/private#1304).
                let question = Component::Text {
                    id: COMPONENT_ID_QUESTION.into(),
                    content: self.t_named("exchange.reconnect.ask"),
                    style: TextStyle::Body,
                    a11y: None,
                };
                // Answered: the buttons go, so a second tap has nothing
                // to press while the AppEngine acts on the first.
                let buttons = if self.reconnect_answer.is_some() {
                    Vec::new()
                } else {
                    vec![
                        self.list_button(
                            RECONNECT_ACCEPT_ACTION_ID,
                            self.t_named("exchange.reconnect.yes"),
                        ),
                        self.list_button(
                            RECONNECT_DECLINE_ACTION_ID,
                            self.t("exchange.reconnect.no"),
                        ),
                    ]
                };
                let answered = buttons.is_empty();
                // The answers come before the camera, which on a scrolling
                // screen is a screen's height of its own.
                let answers = vec![self.exchange_buttons(buttons)];
                let mut screen = self.exchange_screen(title, vec![question], answers, Vec::new());
                if !answered {
                    screen.pinned_list = Some(PinnedList {
                        component_id: EXCHANGE_ACTIONS_ID.into(),
                        lifted: Vec::new(),
                    });
                }
                screen
            }
            RepeatExchange::Waiting => {
                let status = self.repeat_status(
                    COMPONENT_ID_WAITING,
                    self.t_named("exchange.reconnect.waiting_title"),
                    self.t_named("exchange.reconnect.waiting_body"),
                );
                let cancel = self.list_button(CANCEL_ACTION_ID, self.t("action.cancel"));
                self.build_exchange_layout(title, status, vec![cancel])
            }
            RepeatExchange::Ended(ending) if self.session_ended => ScreenModel::new(
                SCREEN_ID,
                title,
                vec![Component::StatusIndicator {
                    id: ending.component_id().into(),
                    icon: Some("person.crop.circle.badge.questionmark".into()),
                    title: self.t_named(ending.text_key()),
                    detail: None,
                    status: Status::Warning,
                    status_label: self.t(Status::Warning.label_key()),
                    a11y: None,
                }],
                vec![ScreenAction {
                    id: DONE_ACTION_ID.into(),
                    label: self.t("action.done"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.done"))),
                }],
            ),
            // Until the session ended the code stays up: the peer reads
            // this phone's DONE from it.
            RepeatExchange::Ended(ending) => {
                let status = Component::Text {
                    id: ending.component_id().into(),
                    content: self.t_named(ending.text_key()),
                    style: TextStyle::Body,
                    a11y: None,
                };
                let done = self.list_button(DONE_ACTION_ID, self.t("action.done"));
                self.build_exchange_layout(title, status, vec![done])
            }
        }
    }
}
