// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One publication per distinct visible-field set (private#1375, B1).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::content::{commitment, record_count};
use super::direction::GroupId;
use super::publish::Publisher;
use crate::card::ContactCard;

pub struct VisibleCard {
    contact: String,
    card: ContactCard,
    publication: Option<Zeroizing<Vec<u8>>>,
}

impl VisibleCard {
    #[cfg(any(test, feature = "testing"))]
    pub fn new(contact: String, card: ContactCard) -> Self {
        Self {
            contact,
            card: super::card::shared_card(&card),
            publication: None,
        }
    }

    pub fn signed(
        contact: String,
        card: ContactCard,
        identity: &crate::identity::Identity,
        device: &crate::crypto::SigningKeyPair,
        revision: u32,
    ) -> Result<Self, GroupsError> {
        let card = super::card::shared_card(&card);
        let group = super::card::group_identifier(&card, identity);
        let publication = super::card::SignedCard::sign(&card, identity, device, group, revision)
            .map_err(|error| match error {
            super::card::CardError::BodySize => GroupsError::BodySize,
            _ => GroupsError::Encoding,
        })?;
        Ok(Self {
            contact,
            card,
            publication: Some(publication),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupsError {
    BodySize,
    Encoding,
    DuplicateContact,
    InconsistentView,
    Exhausted,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audience {
    pub group: GroupId,
    pub update_key: Zeroizing<[u8; 32]>,
    pub commitment: [u8; 31],
}

impl std::fmt::Debug for Audience {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audience")
            .field("group", &self.group)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
struct Group {
    audience: Audience,
    members: BTreeSet<String>,
    // ContactCard omits optional fields; JSON preserves them in a postcard save.
    body: Zeroizing<Vec<u8>>,
    card_json: Zeroizing<Vec<u8>>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Groups {
    views: BTreeMap<Vec<String>, Group>,
    publishers: Vec<Publisher>,
    next_group: GroupId,
}

struct Desired {
    body: Zeroizing<Vec<u8>>,
    card_json: Zeroizing<Vec<u8>>,
    members: BTreeSet<String>,
}

impl Groups {
    /// Replaces the partition atomically in memory. Membership changes rotate
    /// the key even when the body is unchanged, excluding a departed member.
    pub fn reconcile(&mut self, cards: Vec<VisibleCard>) -> Result<(), GroupsError> {
        let desired = partition(cards)?;
        self.validate()?;
        let needed = desired
            .keys()
            .filter(|key| !self.views.contains_key(*key))
            .count();
        let needed = u32::try_from(needed).map_err(|_| GroupsError::Exhausted)?;
        self.next_group
            .checked_add(needed)
            .ok_or(GroupsError::Exhausted)?;
        let mut previous = std::mem::take(&mut self.views);
        let mut publishers: BTreeMap<_, _> = std::mem::take(&mut self.publishers)
            .into_iter()
            .map(|publisher| (publisher.group(), publisher))
            .collect();
        for (fields, desired) in desired {
            let old = previous.remove(&fields);
            let unchanged = old.as_ref().is_some_and(|group| {
                group.members == desired.members
                    && commitment(&group.audience.update_key, &desired.body)
                        == group.audience.commitment
            });
            let id = match old.as_ref() {
                Some(group) => group.audience.group,
                None => {
                    let id = self.next_group;
                    self.next_group += 1;
                    id
                }
            };
            let audience = if unchanged {
                old.as_ref()
                    .map(|group| group.audience.clone())
                    .ok_or(GroupsError::Encoding)?
            } else {
                let key = Zeroizing::new(crate::crypto::random_bytes());
                Audience {
                    group: id,
                    commitment: commitment(&key, &desired.body),
                    update_key: key,
                }
            };
            let publisher = if unchanged {
                publishers.remove(&id).ok_or(GroupsError::Encoding)?
            } else {
                Publisher::for_group(id, *audience.update_key, &desired.body)
            };
            self.publishers.push(publisher);
            self.views.insert(
                fields,
                Group {
                    audience,
                    members: desired.members,
                    body: desired.body,
                    card_json: desired.card_json,
                },
            );
        }
        Ok(())
    }

    pub fn for_contact(&self, contact: &str) -> Option<Audience> {
        self.views
            .values()
            .find(|group| group.members.contains(contact))
            .map(|group| group.audience.clone())
    }

    pub fn card_for_contact(&self, contact: &str) -> Option<ContactCard> {
        self.views
            .values()
            .find(|group| group.members.contains(contact))
            .and_then(|group| serde_json::from_slice(&group.card_json).ok())
    }

    pub fn publications(&self) -> &[Publisher] {
        &self.publishers
    }

    pub fn publications_mut(&mut self) -> &mut [Publisher] {
        &mut self.publishers
    }

    pub fn save(&self) -> Result<Zeroizing<Vec<u8>>, super::saved::SavedError> {
        super::saved::save(super::saved::Kind::Groups, self)
    }

    pub fn load(bytes: &[u8]) -> Result<Self, super::saved::SavedError> {
        let groups: Self = super::saved::load(super::saved::Kind::Groups, bytes)?;
        groups.validate().map_err(|_| super::saved::SavedError)?;
        Ok(groups)
    }

    fn validate(&self) -> Result<(), GroupsError> {
        if self.publishers.len() != self.views.len() {
            return Err(GroupsError::Encoding);
        }
        let mut ids = BTreeSet::new();
        let mut members = BTreeSet::new();
        for group in self.views.values() {
            let audience = &group.audience;
            if audience.group >= self.next_group
                || !ids.insert(audience.group)
                || group.members.is_empty()
                || !group.members.iter().all(|member| members.insert(member))
                || record_count(group.body.len()).is_err()
                || commitment(&audience.update_key, &group.body) != audience.commitment
                || !self.publishers.iter().any(|publisher| {
                    publisher.group() == audience.group && publisher.publishes(&audience.update_key)
                })
            {
                return Err(GroupsError::Encoding);
            }
        }
        Ok(())
    }
}

fn partition(cards: Vec<VisibleCard>) -> Result<BTreeMap<Vec<String>, Desired>, GroupsError> {
    let mut desired: BTreeMap<Vec<String>, Desired> = BTreeMap::new();
    let mut contacts = BTreeSet::new();
    for view in cards {
        if !contacts.insert(view.contact.clone()) {
            return Err(GroupsError::DuplicateContact);
        }
        let mut fields: Vec<_> = view
            .card
            .fields()
            .iter()
            .map(|field| field.id().to_string())
            .collect();
        fields.sort();
        let card_json =
            Zeroizing::new(serde_json::to_vec(&view.card).map_err(|_| GroupsError::Encoding)?);
        let body = view.publication.unwrap_or_else(|| card_json.clone());
        record_count(body.len()).map_err(|_| GroupsError::BodySize)?;
        if let Some(group) = desired.get_mut(&fields) {
            if group.body != body {
                return Err(GroupsError::InconsistentView);
            }
            group.members.insert(view.contact);
        } else {
            desired.insert(
                fields,
                Desired {
                    body,
                    card_json,
                    members: [view.contact].into(),
                },
            );
        }
    }
    Ok(desired)
}

// INLINE_TEST_REQUIRED: corrupt private saved-state fixtures to test bounded decoding.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn a_saved_group_with_an_invalid_body_size_is_refused_without_panicking() {
        for length in [0, super::super::content::MAX_BODY_BYTES + 1] {
            let mut groups = Groups::default();
            groups
                .reconcile(vec![VisibleCard::new(
                    "bob".into(),
                    ContactCard::new("Alice"),
                )])
                .unwrap();
            groups.views.values_mut().next().unwrap().body = Zeroizing::new(vec![0; length]);
            let saved = groups.save().unwrap();
            assert!(matches!(
                Groups::load(&saved),
                Err(super::super::saved::SavedError)
            ));
        }
    }
}
