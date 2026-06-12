// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact list engine — searchable list of all contacts with group filtering.

use std::collections::HashMap;

use crate::ui::*;

/// An `Item` paired with its engine-internal search index.
///
/// `Item` is the wire shape (Wire Humble: presentation only). The
/// per-contact searchable text — phone numbers, emails, etc. — is
/// engine input that never crosses to the wire. Callers wrap each
/// `Item` they want indexed, or pass `searchable: vec![]` for items
/// they don't want returned by search.
#[derive(Clone, Debug)]
pub struct IndexedItem {
    pub item: Item,
    pub searchable: Vec<String>,
}

impl IndexedItem {
    pub fn new(item: Item, searchable: Vec<String>) -> Self {
        Self { item, searchable }
    }
}

impl From<Item> for IndexedItem {
    fn from(item: Item) -> Self {
        Self {
            item,
            searchable: Vec::new(),
        }
    }
}

/// Window length for windowed `Component::List` emissions. Filtered
/// sets at or under this size are emitted whole (the exact unwindowed
/// wire shape); larger sets are sliced so a 10k-contact list never puts
/// a multi-MB emission on the wire
/// (`2026-06-11-contacts-list-eager-render-anr` Track B).
const DEFAULT_LIST_WINDOW: usize = 200;

/// Contact list engine — full contact list with search and group filtering.
#[derive(Clone, Debug)]
pub struct ContactListEngine {
    all_contacts: Vec<IndexedItem>,
    search_query: String,
    /// Active group filter: None = show all, Some(group_id) = show only members.
    group_filter: Option<String>,
    /// Available groups: (group_id, group_name).
    available_groups: Vec<(String, String)>,
    /// Group memberships: group_id -> list of contact_ids.
    group_memberships: HashMap<String, Vec<String>>,
    /// Faceted-search opt-ins (ADR-051): match the query against tags /
    /// comment / place in addition to the name. Default off → plain name
    /// search (behaviour unchanged).
    facet_tags: bool,
    facet_comment: bool,
    facet_place: bool,
    /// When `Some`, faceted mode is active and the list is restricted to
    /// these contact ids (computed by the AppEngine intercept via
    /// `Vauchi::search_contacts_faceted`). `None` → plain in-memory search.
    faceted_ids: Option<Vec<String>>,
    /// Requested window start within the filtered set. Clamped at render
    /// time so a filter change that shrinks the set can never emit past
    /// its end; reset to the top whenever the filtered set itself changes
    /// (search, facets, group filter).
    window_offset: usize,
}

impl ContactListEngine {
    pub fn new(contacts: Vec<IndexedItem>) -> Self {
        Self {
            all_contacts: contacts,
            search_query: String::new(),
            group_filter: None,
            available_groups: Vec::new(),
            group_memberships: HashMap::new(),
            facet_tags: false,
            facet_comment: false,
            facet_place: false,
            faceted_ids: None,
            window_offset: 0,
        }
    }

    /// Create engine with group information for filtering.
    pub fn with_groups(
        contacts: Vec<IndexedItem>,
        groups: Vec<(String, String)>,
        memberships: HashMap<String, Vec<String>>,
    ) -> Self {
        Self {
            all_contacts: contacts,
            search_query: String::new(),
            group_filter: None,
            available_groups: groups,
            group_memberships: memberships,
            facet_tags: false,
            facet_comment: false,
            facet_place: false,
            faceted_ids: None,
            window_offset: 0,
        }
    }

    /// True if any annotation facet is enabled.
    pub fn any_facet(&self) -> bool {
        self.facet_tags || self.facet_comment || self.facet_place
    }

    /// Current facet opt-ins as `(tags, comment, place)`.
    pub fn facet_flags(&self) -> (bool, bool, bool) {
        (self.facet_tags, self.facet_comment, self.facet_place)
    }

    /// The current search query.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Flip one facet opt-in by id (`tags` | `comment` | `place`).
    pub fn toggle_facet(&mut self, id: &str) {
        match id {
            "tags" => self.facet_tags = !self.facet_tags,
            "comment" => self.facet_comment = !self.facet_comment,
            "place" => self.facet_place = !self.facet_place,
            _ => {}
        }
        self.window_offset = 0;
    }

    /// Set the search query (used by the intercept when faceting).
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.window_offset = 0;
    }

    /// Set (or clear) the faceted result restriction. `Some` activates
    /// faceted mode (the list shows exactly these ids, intersected with the
    /// group filter); `None` reverts to plain in-memory name search.
    pub fn set_faceted_ids(&mut self, ids: Option<Vec<String>>) {
        self.faceted_ids = ids;
        self.window_offset = 0;
    }

    fn search_facets_toggle(&self) -> Component {
        let item = |id: &str, label: &str, selected: bool| ToggleItem {
            id: id.into(),
            label: label.into(),
            selected,
            subtitle: None,
            a11y: Some(A11y {
                label: Some(format!(
                    "Search {label}, {}",
                    if selected { "on" } else { "off" }
                )),
                hint: Some("Double tap to toggle".into()),
                role: Some(AccessibilityRole::Toggle),
            }),
            info_key: None,
        };
        Component::ToggleList {
            id: "search_facets".into(),
            label: "Also search".into(),
            items: vec![
                item("tags", "Tags", self.facet_tags),
                item("comment", "Notes", self.facet_comment),
                item("place", "Places", self.facet_place),
            ],
            a11y: Some(A11y {
                label: Some("Also search in".into()),
                hint: Some("Choose which annotations the search matches.".into()),
                role: None,
            }),
        }
    }

    fn filtered_contacts(&self) -> Vec<&Item> {
        let faceted: Option<std::collections::HashSet<&str>> = self
            .faceted_ids
            .as_ref()
            .map(|ids| ids.iter().map(String::as_str).collect());
        let query_lower = self.search_query.to_lowercase();
        // Plain name search only applies when not in faceted mode (core has
        // already matched the query against name + the enabled facets).
        let search_active = faceted.is_none() && !query_lower.is_empty();

        self.all_contacts
            .iter()
            .filter(|c| {
                if let Some(group_id) = &self.group_filter {
                    let member_ids = self.group_memberships.get(group_id);
                    if !member_ids
                        .map(|ids| ids.contains(&c.item.id))
                        .unwrap_or(false)
                    {
                        return false;
                    }
                }
                if let Some(set) = &faceted {
                    return set.contains(c.item.id.as_str());
                }
                if search_active {
                    let name_match = c.item.name.to_lowercase().contains(&query_lower);
                    let field_match = c
                        .searchable
                        .iter()
                        .any(|f| f.to_lowercase().contains(&query_lower));
                    return name_match || field_match;
                }
                true
            })
            .map(|c| &c.item)
            .collect()
    }
}

impl WorkflowEngine for ContactListEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::ContactList {
            query: self.search_query().to_string(),
            any_facet: self.any_facet(),
            facets: self.facet_flags(),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::ContactList(update) = update else {
            return false;
        };
        match update {
            crate::ui::ContactListUpdate::ToggleFacet(id) => self.toggle_facet(&id),
            crate::ui::ContactListUpdate::SearchQuery(query) => self.set_search_query(query),
            crate::ui::ContactListUpdate::FacetedIds(ids) => self.set_faceted_ids(ids),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        let mut actions = vec![ScreenAction {
            id: "add_contact".into(),
            label: "Add Contact".into(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        }];

        // Add group filter actions
        for (gid, gname) in &self.available_groups {
            actions.push(ScreenAction {
                id: format!("filter_group:{gid}"),
                label: gname.clone(),
                style: if self.group_filter.as_deref() == Some(gid) {
                    ActionStyle::Primary
                } else {
                    ActionStyle::Secondary
                },
                enabled: true,
                a11y: None,
            });
        }

        // Add clear filter action when a filter is active
        if self.group_filter.is_some() {
            actions.push(ScreenAction {
                id: "filter_group_clear".into(),
                label: "All Contacts".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        }

        // Empty state: when the user has no contacts at all (not just
        // empty search results), show guidance encouraging first exchange.
        let filtered = self.filtered_contacts();
        let components = if self.all_contacts.is_empty() {
            vec![Component::InfoPanel {
                id: "empty_state".into(),
                icon: Some("people".into()),
                title: "No contacts yet".into(),
                items: vec![InfoItem {
                    icon: Some("exchange".into()),
                    title: "Exchange cards in person".into(),
                    detail: "Meet someone nearby and share your contact card securely.".into(),
                }],
                a11y: None,
            }]
        } else {
            let total = filtered.len();
            let (window_items, total_count, offset, window) = if total > DEFAULT_LIST_WINDOW {
                let offset = self.window_offset.min(total - DEFAULT_LIST_WINDOW);
                (
                    &filtered[offset..offset + DEFAULT_LIST_WINDOW],
                    total,
                    offset,
                    DEFAULT_LIST_WINDOW,
                )
            } else {
                (&filtered[..], 0, 0, 0)
            };
            vec![
                self.search_facets_toggle(),
                Component::List {
                    id: "contacts".into(),
                    items: window_items.iter().map(|&i| i.clone()).collect(),
                    searchable: true,
                    total_count,
                    offset,
                    window,
                },
            ]
        };

        // Archived contacts link
        actions.push(ScreenAction {
            id: "view_archived".into(),
            label: "Archived Contacts".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        // Find duplicates action
        actions.push(ScreenAction {
            id: "find_duplicates".into(),
            label: "Find Duplicates".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        // Add exchange shortcut when empty
        if self.all_contacts.is_empty() {
            actions.insert(
                0,
                ScreenAction {
                    id: "go_exchange".into(),
                    label: "Exchange Now".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
            );
        }

        ScreenModel {
            screen_id: "contact_list".into(),
            title: "Contacts".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            // The list owns scrolling lazily; eager rendering of 10k
            // rows crashed the mobile renderers
            // (2026-06-11-contacts-list-eager-render-anr).
            layout: crate::ui::screen::ScreenLayout::Pinned,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::SearchChanged { query, .. } => {
                self.set_search_query(query);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "search_facets" => {
                self.toggle_facet(&item_id);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { item_id, .. } => ActionResult::OpenContact {
                contact_id: item_id,
            },
            // `add_contact` is intercepted by `AppEngine` and routed to
            // the Exchange screen (see `app_engine/mod.rs`). If the engine
            // is driven standalone (e.g. in a unit test) the action falls
            // through to the `_` arm and produces a harmless screen
            // refresh — the engine itself has no navigation authority.
            UserAction::ActionPressed { ref action_id }
                if action_id.starts_with("filter_group:") =>
            {
                let group_id = action_id.strip_prefix("filter_group:").unwrap().to_string();
                self.group_filter = Some(group_id);
                self.window_offset = 0;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { ref action_id } if action_id == "filter_group_clear" => {
                self.group_filter = None;
                self.window_offset = 0;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListWindowRequested {
                component_id,
                offset,
            } if component_id == "contacts" => {
                self.window_offset = offset;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemAction {
                item_id, action_id, ..
            } => {
                // Validate that `action_id` was one we actually offered
                // for this item. Stale/forged action ids drop to a
                // no-op screen refresh rather than performing a random
                // mutation on the wrong contact.
                let offered = self
                    .all_contacts
                    .iter()
                    .find(|c| c.item.id == item_id)
                    .and_then(|c| c.item.actions.iter().find(|a| a.id == action_id))
                    .map(|a| a.kind);
                if let Some(kind) = offered
                    && let Some(contact_kind) = contact_action_kind_from(kind)
                {
                    return ActionResult::ContactAction {
                        contact_id: item_id,
                        kind: contact_kind,
                    };
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

fn contact_action_kind_from(kind: ListItemActionKind) -> Option<ContactActionKind> {
    match kind {
        ListItemActionKind::Archive => Some(ContactActionKind::Archive),
        ListItemActionKind::Unarchive => Some(ContactActionKind::Unarchive),
        ListItemActionKind::Hide => Some(ContactActionKind::Hide),
        ListItemActionKind::Unhide => Some(ContactActionKind::Unhide),
        ListItemActionKind::Delete => Some(ContactActionKind::Delete),
        ListItemActionKind::Undelete => Some(ContactActionKind::Undelete),
        ListItemActionKind::Custom => None,
    }
}

// INLINE_TEST_REQUIRED: tests exercise the private `contact_action_kind_from`
// helper. Moving them to tests/ would require making the helper public, which
// is a wider surface than the invariant they protect.
#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, actions: Vec<ListItemAction>) -> IndexedItem {
        IndexedItem::from(Item {
            id: id.into(),
            name: format!("Contact {id}"),
            subtitle: None,
            avatar_initials: "C".into(),
            status: None,
            actions,
            a11y: None,
        })
    }

    fn archive_action() -> ListItemAction {
        ListItemAction {
            id: "archive".into(),
            label: "Archive".into(),
            kind: ListItemActionKind::Archive,
            destructive: false,
        }
    }

    /// Wire-contract window length (Track B,
    /// `2026-06-11-contacts-list-eager-render-anr`): asserted as a
    /// literal so the tests pin the spec, not the implementation.
    const WINDOW: usize = 200;

    fn contacts(n: usize) -> Vec<IndexedItem> {
        (0..n).map(|i| item(&format!("c{i}"), vec![])).collect()
    }

    fn list_window(screen: &ScreenModel) -> (Vec<String>, usize, usize, usize) {
        match screen
            .components
            .iter()
            .find(|c| matches!(c, Component::List { .. }))
        {
            Some(Component::List {
                items,
                total_count,
                offset,
                window,
                ..
            }) => (
                items.iter().map(|i| i.id.clone()).collect(),
                *total_count,
                *offset,
                *window,
            ),
            other => panic!("expected a List component, got {other:?}"),
        }
    }

    fn updated_screen(result: ActionResult) -> ScreenModel {
        match result {
            ActionResult::UpdateScreen(screen) => screen,
            other => panic!("expected UpdateScreen, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn large_list_emits_first_window_with_total_count() {
        let engine = ContactListEngine::new(contacts(250));
        let (ids, total, offset, window) = list_window(&engine.current_screen());
        assert_eq!(total, 250);
        assert_eq!(offset, 0);
        assert_eq!(window, WINDOW);
        assert_eq!(ids.len(), WINDOW);
        assert_eq!(ids[0], "c0");
        assert_eq!(ids[WINDOW - 1], "c199");
    }

    // @internal
    #[test]
    fn small_list_stays_unwindowed() {
        // At or under one window the emission keeps the exact
        // pre-windowing wire shape (zeros are skip-serialized).
        let engine = ContactListEngine::new(contacts(WINDOW));
        let (ids, total, offset, window) = list_window(&engine.current_screen());
        assert_eq!((total, offset, window), (0, 0, 0));
        assert_eq!(ids.len(), WINDOW);
    }

    // @internal
    #[test]
    fn window_request_moves_window() {
        let mut engine = ContactListEngine::new(contacts(500));
        let screen = updated_screen(engine.handle_action(UserAction::ListWindowRequested {
            component_id: "contacts".into(),
            offset: 150,
        }));
        let (ids, total, offset, window) = list_window(&screen);
        assert_eq!(total, 500);
        assert_eq!(offset, 150);
        assert_eq!(window, WINDOW);
        assert_eq!(ids[0], "c150");
        assert_eq!(ids[WINDOW - 1], "c349");
    }

    // @internal
    #[test]
    fn window_request_clamps_to_last_full_window() {
        let mut engine = ContactListEngine::new(contacts(500));
        let screen = updated_screen(engine.handle_action(UserAction::ListWindowRequested {
            component_id: "contacts".into(),
            offset: 9999,
        }));
        let (ids, total, offset, window) = list_window(&screen);
        assert_eq!(total, 500);
        assert_eq!(offset, 300);
        assert_eq!(window, WINDOW);
        assert_eq!(ids[0], "c300");
        assert_eq!(ids[WINDOW - 1], "c499");
    }

    // @internal
    #[test]
    fn window_request_for_other_component_keeps_window() {
        let mut engine = ContactListEngine::new(contacts(500));
        let screen = updated_screen(engine.handle_action(UserAction::ListWindowRequested {
            component_id: "search_facets".into(),
            offset: 300,
        }));
        let (ids, _, offset, _) = list_window(&screen);
        assert_eq!(offset, 0);
        assert_eq!(ids[0], "c0");
    }

    // @internal
    #[test]
    fn search_change_resets_window_offset() {
        let mut engine = ContactListEngine::new(contacts(500));
        let _ = engine.handle_action(UserAction::ListWindowRequested {
            component_id: "contacts".into(),
            offset: 300,
        });
        // Every fixture contact matches "contact" — the filtered set is
        // unchanged but the result window must restart at the top.
        let screen = updated_screen(engine.handle_action(UserAction::SearchChanged {
            component_id: "contacts".into(),
            query: "contact".into(),
        }));
        let (ids, total, offset, _) = list_window(&screen);
        assert_eq!(total, 500);
        assert_eq!(offset, 0);
        assert_eq!(ids[0], "c0");
    }

    // @internal
    #[test]
    fn group_filter_change_resets_window_offset() {
        let groups = vec![("work".to_string(), "Work".to_string())];
        let mut memberships: HashMap<String, Vec<String>> = HashMap::new();
        memberships.insert("work".into(), (0..300).map(|i| format!("c{i}")).collect());
        let mut engine = ContactListEngine::with_groups(contacts(500), groups, memberships);
        let _ = engine.handle_action(UserAction::ListWindowRequested {
            component_id: "contacts".into(),
            offset: 300,
        });
        let screen = updated_screen(engine.handle_action(UserAction::ActionPressed {
            action_id: "filter_group:work".into(),
        }));
        let (ids, total, offset, _) = list_window(&screen);
        assert_eq!(total, 300);
        assert_eq!(offset, 0);
        assert_eq!(ids[0], "c0");
    }

    mod windowing_properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            // @internal
            #[test]
            fn windowed_emission_is_a_contiguous_slice_of_the_filtered_set(
                n in 1usize..450,
                requested in 0usize..1000,
            ) {
                let mut engine = ContactListEngine::new(contacts(n));
                let _ = engine.handle_action(UserAction::ListWindowRequested {
                    component_id: "contacts".into(),
                    offset: requested,
                });
                let (ids, total, offset, window) = list_window(&engine.current_screen());
                if n > WINDOW {
                    prop_assert_eq!(total, n);
                    prop_assert_eq!(window, WINDOW);
                    prop_assert_eq!(window, ids.len());
                    prop_assert!(offset + window <= n);
                    for (k, id) in ids.iter().enumerate() {
                        prop_assert_eq!(id, &format!("c{}", offset + k));
                    }
                } else {
                    prop_assert_eq!((total, offset, window), (0, 0, 0));
                    prop_assert_eq!(ids.len(), n);
                }
            }
        }
    }

    // @internal
    #[test]
    fn contacts_screen_uses_pinned_layout() {
        // The list component owns scrolling (lazy) so 10k contacts never
        // render eagerly (2026-06-11-contacts-list-eager-render-anr).
        let engine = ContactListEngine::new(vec![item("c1", vec![])]);
        assert_eq!(
            engine.current_screen().layout,
            crate::ui::screen::ScreenLayout::Pinned
        );
    }

    // @internal
    #[test]
    fn empty_contacts_screen_keeps_pinned_layout() {
        // One stable layout per screen — the empty InfoPanel state must
        // not flip the renderer between scroll hosts.
        let engine = ContactListEngine::new(vec![]);
        assert_eq!(
            engine.current_screen().layout,
            crate::ui::screen::ScreenLayout::Pinned
        );
    }

    // @internal
    #[test]
    fn list_item_action_archive_returns_contact_action_archive() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "archive".into(),
        });
        assert_eq!(
            result,
            ActionResult::ContactAction {
                contact_id: "c1".into(),
                kind: ContactActionKind::Archive,
            }
        );
    }

    // @internal
    #[test]
    fn list_item_action_unknown_action_id_is_noop() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "hide".into(), // not in item's actions
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn list_item_action_unknown_item_id_is_noop() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c999".into(),
            action_id: "archive".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn list_item_action_custom_kind_is_noop() {
        let mut engine = ContactListEngine::new(vec![item(
            "c1",
            vec![ListItemAction {
                id: "pin".into(),
                label: "Pin".into(),
                kind: ListItemActionKind::Custom,
                destructive: false,
            }],
        )]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "pin".into(),
        });
        // Custom kinds require dedicated handling — engine refuses rather
        // than guessing, so screen refreshes without a mutation.
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn all_kinds_round_trip() {
        // Every ListItemActionKind (except Custom) must map to a
        // ContactActionKind — otherwise the engine silently drops it.
        use ListItemActionKind::*;
        for (k, expected) in [
            (Archive, Some(ContactActionKind::Archive)),
            (Unarchive, Some(ContactActionKind::Unarchive)),
            (Hide, Some(ContactActionKind::Hide)),
            (Unhide, Some(ContactActionKind::Unhide)),
            (Delete, Some(ContactActionKind::Delete)),
            (Undelete, Some(ContactActionKind::Undelete)),
            (Custom, None),
        ] {
            assert_eq!(contact_action_kind_from(k), expected, "kind={k:?}");
        }
    }
}
