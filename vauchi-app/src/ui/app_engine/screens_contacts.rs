// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contacts-family screen factory — split from `screens.rs` (see
//! `create_engine`), which dispatches the matching `AppScreen`
//! variants here.

use super::AppEngine;
use super::AppScreen;
use crate::ui::archived_contacts::ArchivedContactsEngine;
use crate::ui::component::{A11y, Field, Item, UiFieldVisibility, initials};
use crate::ui::contact_detail::{ContactDetailEngine, ContactNotFoundEngine, DeliverySummary};
use crate::ui::contact_detail_rules::{ContactPlace, ContactTag};
use crate::ui::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use crate::ui::contact_limit::ContactLimitEngine;
use crate::ui::contact_list::ContactListEngine;
use crate::ui::contact_merge::{ContactMergeEngine, MergePreview};
use crate::ui::contact_visibility::ContactVisibilityEngine;
use crate::ui::duplicate_detection::{DuplicateDetectionEngine, DuplicatePair};
use crate::ui::engine::WorkflowEngine;
use crate::ui::fingerprint_verify::FingerprintVerifyEngine;
use std::collections::HashMap;
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_contacts_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        render_context: &crate::ui::RenderContext,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Contacts => {
                let contacts = Self::load_contact_items(vauchi, render_context.resolved_locale());
                let all_groups = vauchi.list_groups().unwrap_or_default();
                if all_groups.is_empty() {
                    Box::new(
                        ContactListEngine::new(contacts)
                            .with_locale(render_context.resolved_locale()),
                    )
                } else {
                    let groups: Vec<(String, String)> = all_groups
                        .iter()
                        .map(|g| (g.id().to_string(), g.name().to_string()))
                        .collect();
                    let mut memberships = HashMap::new();
                    for g in &all_groups {
                        let member_ids: Vec<String> = contacts
                            .iter()
                            .filter(|c| g.contains_contact(&c.item.id))
                            .map(|c| c.item.id.clone())
                            .collect();
                        memberships.insert(g.id().to_string(), member_ids);
                    }
                    Box::new(
                        ContactListEngine::with_groups(contacts, groups, memberships)
                            .with_locale(render_context.resolved_locale()),
                    )
                }
            }
            AppScreen::ContactDetail { contact_id } => match vauchi.get_contact(contact_id) {
                Ok(Some(contact)) => {
                    let fields: Vec<Field> = contact
                        .card()
                        .fields()
                        .iter()
                        .map(|f| {
                            let field_type_str = format!("{:?}", f.field_type());
                            Field {
                                id: f.id().to_string(),
                                icon: crate::ui::component::icon_for_field_type(&field_type_str)
                                    .into(),
                                field_type: field_type_str,
                                label: f.label().to_string(),
                                value: f.value().to_string(),
                                visibility: UiFieldVisibility::Shown,
                                a11y: None,
                            }
                        })
                        .collect();
                    let status = if vauchi.is_contact_revoked(contact.id()) {
                        Some("Deleted their identity".into())
                    } else if contact.has_recovered() && !contact.is_fingerprint_verified() {
                        Some("Recovered — re-verify recommended".into())
                    } else {
                        None
                    };
                    let item = Item {
                        id: contact.id().to_string(),
                        name: contact.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(contact.display_name()),
                        status,
                        actions: vec![],
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", contact.display_name())),
                            hint: Some(crate::i18n::get_string(
                                render_context.resolved_locale(),
                                "contact_detail.double_tap_to_view_hint",
                            )),
                            role: None,
                        }),
                    };

                    // Load personal note (stored as raw UTF-8 bytes by the app layer)
                    let personal_note = vauchi
                        .load_personal_notes(contact_id)
                        .ok()
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .unwrap_or_default();

                    // Load per-field notes — convert raw bytes to UTF-8 strings
                    let field_notes: HashMap<String, String> = vauchi
                        .load_contact_field_notes(contact_id)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|(field_id, bytes)| {
                            String::from_utf8(bytes).ok().map(|s| (field_id, s))
                        })
                        .collect();

                    // Build shared info (my card as seen by this contact)
                    let shared_info = Self::build_shared_info(vauchi, contact_id);

                    let trust_level = contact.trust_level().to_string();
                    let trust_level_enum = contact.trust_level();
                    let proposal_trusted = contact.is_proposal_trusted();
                    let is_hidden = contact.is_hidden();
                    let is_imported = contact.is_imported();
                    let is_verified = contact.is_fingerprint_verified();
                    let fingerprint = contact.fingerprint();
                    let is_recovery_trusted = contact.is_recovery_trusted();

                    // Reciprocity status (design spec §6.3)
                    use vauchi_core::exchange::reciprocity::Reciprocity;
                    let reciprocity_status = match contact.reciprocity(0) {
                        Reciprocity::Pending => "Awaiting confirmation".to_string(),
                        Reciprocity::Unreciprocated => "May not have your card".to_string(),
                        _ => String::new(),
                    };

                    // Delivery status summary (J1: update propagation)
                    let delivery_summary = vauchi
                        .get_delivery_status_for_contact(contact_id)
                        .ok()
                        .map(|records| {
                            use vauchi_core::storage::DeliveryStatus;
                            let total = records.len();
                            let delivered = records
                                .iter()
                                .filter(|r| matches!(r.status, DeliveryStatus::Delivered))
                                .count();
                            let failed = records
                                .iter()
                                .filter(|r| {
                                    matches!(
                                        r.status,
                                        DeliveryStatus::Failed { .. } | DeliveryStatus::Expired
                                    )
                                })
                                .count();
                            let pending = total - delivered - failed;
                            DeliverySummary {
                                total,
                                delivered,
                                pending,
                                failed,
                            }
                        });

                    let avatar_data = contact.card().avatar().map(|a| a.to_vec());

                    // Owner-private tags for this contact (ADR-051). Reduced to
                    // the UI-shaped {id, name} the renderer needs.
                    let tags: Vec<ContactTag> = vauchi
                        .tags_for_contact(contact_id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| ContactTag {
                            id: t.id,
                            name: t.name,
                        })
                        .collect();

                    // Recorded exchange place (ADR-051): resolve the linked
                    // named place to its name for display, if any.
                    let exchange_place =
                        vauchi
                            .exchange_location(contact_id)
                            .ok()
                            .flatten()
                            .map(|loc| {
                                let name = loc.place_id.as_ref().and_then(|pid| {
                                    vauchi
                                        .list_places()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .find(|p| &p.id == pid)
                                        .map(|p| p.name)
                                });
                                ContactPlace { name }
                            });

                    let build_engine = |engine: ContactDetailEngine| {
                        let mut e = engine
                            .with_avatar_data(avatar_data)
                            .with_tags(tags)
                            .with_exchange_place(exchange_place)
                            .with_field_notes(field_notes)
                            .with_trust(trust_level, proposal_trusted)
                            .with_reciprocity(reciprocity_status)
                            .with_hidden(is_hidden)
                            .with_imported(is_imported)
                            .with_verification(is_verified, trust_level_enum)
                            .with_fingerprint(fingerprint)
                            .with_recovery_trusted(is_recovery_trusted)
                            .with_locale(render_context.resolved_locale());
                        if let Some(summary) = delivery_summary
                            && summary.total > 0
                        {
                            e = e.with_delivery_summary(summary);
                        }
                        e
                    };

                    match shared_info {
                        Some(info) => {
                            Box::new(build_engine(ContactDetailEngine::with_shared_info(
                                item,
                                fields,
                                info,
                                personal_note,
                            )))
                        }
                        None => Box::new(build_engine(ContactDetailEngine::new(
                            item,
                            fields,
                            personal_note,
                        ))),
                    }
                }
                _ => Box::new(
                    ContactNotFoundEngine::new(contact_id.clone())
                        .with_locale(render_context.resolved_locale()),
                ),
            },
            AppScreen::ContactVisibility { contact_id } => {
                let (name, fields) = match vauchi.get_contact(contact_id) {
                    Ok(Some(contact)) => {
                        let name = contact.display_name().to_string();
                        let items = contact
                            .card()
                            .fields()
                            .iter()
                            .map(|f| crate::ui::component::ToggleItem {
                                id: f.id().to_string(),
                                label: f.label().to_string(),
                                selected: true,
                                subtitle: None,
                                a11y: None,
                                info_key: None,
                            })
                            .collect();
                        (name, items)
                    }
                    _ => (
                        format!("Contact {}", &contact_id[..8.min(contact_id.len())]),
                        vec![],
                    ),
                };
                Box::new(
                    ContactVisibilityEngine::new(name, fields)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            AppScreen::ContactEdit { contact_id } => match vauchi.get_contact(contact_id) {
                Ok(Some(contact)) => {
                    let fields = contact
                        .card()
                        .fields()
                        .iter()
                        .map(|f| EditableField {
                            id: f.id().to_string(),
                            field_type: format!("{:?}", f.field_type()),
                            label: f.label().to_string(),
                            value: f.value().to_string(),
                            visible_to_groups: vec![],
                            shown: true,
                        })
                        .collect();
                    let editable = EditableContact {
                        display_name: contact.display_name().to_string(),
                        fields,
                    };
                    let avatar_data = vauchi
                        .own_card()
                        .ok()
                        .flatten()
                        .and_then(|c| c.avatar().map(|a| a.to_vec()));
                    Box::new(
                        ContactEditEngine::new(editable, vec![])
                            .with_avatar_data(avatar_data)
                            .with_locale(render_context.resolved_locale()),
                    )
                }
                _ => Box::new(
                    ContactNotFoundEngine::new(contact_id.clone())
                        .with_locale(render_context.resolved_locale()),
                ),
            },
            AppScreen::ContactDuplicates => {
                let pairs = vauchi.find_duplicates().unwrap_or_default();
                let ui_pairs: Vec<_> = pairs
                    .iter()
                    .map(|p| {
                        let c1 = vauchi.get_contact(&p.id1).ok().flatten();
                        let c2 = vauchi.get_contact(&p.id2).ok().flatten();
                        let name1 = c1
                            .as_ref()
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| p.id1.clone());
                        let name2 = c2
                            .as_ref()
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| p.id2.clone());
                        // Cross-kind detection drives the merge-vs-delete-imported
                        // routing in intercept; populate even when one side is
                        // missing (treat missing as not-imported, mirrors get_contact
                        // failure path elsewhere).
                        let is_imported_1 = c1.as_ref().map(|c| c.is_imported()).unwrap_or(false);
                        let is_imported_2 = c2.as_ref().map(|c| c.is_imported()).unwrap_or(false);
                        DuplicatePair {
                            id1: p.id1.clone(),
                            name1,
                            is_imported_1,
                            id2: p.id2.clone(),
                            name2,
                            is_imported_2,
                            similarity: p.similarity,
                        }
                    })
                    .collect();
                Box::new(
                    DuplicateDetectionEngine::new(ui_pairs)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            AppScreen::ArchivedContacts => {
                let archived = vauchi
                    .list_archived_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (c.id().to_string(), c.display_name().to_string()))
                    .collect();
                Box::new(
                    ArchivedContactsEngine::new(archived)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            AppScreen::ContactMerge {
                primary_name,
                primary_fields,
                secondary_name,
                secondary_fields,
            } => Box::new(
                ContactMergeEngine::new(MergePreview {
                    primary_name: primary_name.clone(),
                    primary_fields: primary_fields.clone(),
                    secondary_name: secondary_name.clone(),
                    secondary_fields: secondary_fields.clone(),
                })
                .with_locale(render_context.resolved_locale()),
            ),
            AppScreen::ContactLimit => {
                let contact_count = vauchi.list_contacts().map(|c| c.len()).unwrap_or(0);
                Box::new(
                    ContactLimitEngine::new(contact_count, 0)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            AppScreen::VerifyFingerprint { contact_id } => {
                let contact = vauchi.get_contact(contact_id).ok().flatten();
                let their_fp = contact
                    .as_ref()
                    .map(|c| c.fingerprint())
                    .unwrap_or_default();
                let our_fp = vauchi.own_fingerprint().unwrap_or_default();
                let is_verified = contact
                    .as_ref()
                    .map(|c| c.is_fingerprint_verified())
                    .unwrap_or(false);
                Box::new(
                    FingerprintVerifyEngine::new(contact_id, &their_fp, &our_fp, is_verified)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            other => unreachable!("non-contacts screen {other:?} routed to contacts factory"),
        }
    }
}
