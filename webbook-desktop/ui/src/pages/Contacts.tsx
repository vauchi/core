import { createResource, createSignal, For, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface ContactInfo {
  id: string
  display_name: string
  verified: boolean
}

interface FieldInfo {
  id: string
  field_type: string
  label: string
  value: string
}

interface ContactDetails {
  id: string
  display_name: string
  verified: boolean
  fields: FieldInfo[]
}

interface VisibilityLevel {
  type: 'everyone' | 'nobody' | 'contacts'
  ids?: string[]
}

interface FieldVisibilityInfo {
  field_id: string
  field_label: string
  field_type: string
  visibility: VisibilityLevel
  can_see: boolean
}

interface ContactsProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings' | 'devices' | 'recovery') => void
}

async function fetchContacts(): Promise<ContactInfo[]> {
  return await invoke('list_contacts')
}

function Contacts(props: ContactsProps) {
  const [contacts, { refetch }] = createResource(fetchContacts)
  const [selectedContact, setSelectedContact] = createSignal<ContactDetails | null>(null)
  const [showDeleteConfirm, setShowDeleteConfirm] = createSignal(false)
  const [showVisibility, setShowVisibility] = createSignal(false)
  const [visibilityRules, setVisibilityRules] = createSignal<FieldVisibilityInfo[]>([])
  const [error, setError] = createSignal('')

  const openContactDetail = async (contactId: string) => {
    try {
      const details = await invoke('get_contact', { id: contactId }) as ContactDetails
      setSelectedContact(details)
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }

  const closeDetail = () => {
    setSelectedContact(null)
    setShowDeleteConfirm(false)
    setShowVisibility(false)
    setVisibilityRules([])
    setError('')
  }

  const loadVisibilityRules = async (contactId: string) => {
    try {
      const rules = await invoke('get_visibility_rules', { contactId }) as FieldVisibilityInfo[]
      setVisibilityRules(rules)
      setShowVisibility(true)
    } catch (e) {
      setError(String(e))
    }
  }

  const toggleFieldVisibility = async (fieldId: string, currentCanSee: boolean) => {
    const contact = selectedContact()
    if (!contact) return

    try {
      const newVisibility: VisibilityLevel = currentCanSee
        ? { type: 'nobody' }
        : { type: 'everyone' }

      await invoke('set_field_visibility', {
        contactId: contact.id,
        fieldId,
        visibility: newVisibility
      })

      // Reload visibility rules
      await loadVisibilityRules(contact.id)
    } catch (e) {
      setError(String(e))
    }
  }

  const handleDelete = async () => {
    const contact = selectedContact()
    if (!contact) return

    try {
      await invoke('remove_contact', { id: contact.id })
      setSelectedContact(null)
      setShowDeleteConfirm(false)
      refetch()
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div class="page contacts">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Contacts</h1>
      </header>

      <div class="contacts-list">
        <For each={contacts()}>
          {(contact) => (
            <div class="contact-item" onClick={() => openContactDetail(contact.id)}>
              <div class="contact-avatar">
                {contact.display_name.charAt(0).toUpperCase()}
              </div>
              <div class="contact-info">
                <span class="contact-name">{contact.display_name}</span>
                <span class="contact-status">
                  {contact.verified ? '✓ Verified' : 'Not verified'}
                </span>
              </div>
            </div>
          )}
        </For>

        {contacts()?.length === 0 && (
          <div class="empty-state">
            <p>No contacts yet</p>
            <button onClick={() => props.onNavigate('exchange')}>
              Exchange with someone
            </button>
          </div>
        )}
      </div>

      {/* Contact Detail Dialog */}
      <Show when={selectedContact()}>
        <div class="dialog-overlay" onClick={closeDetail}>
          <div class="dialog contact-detail" onClick={(e) => e.stopPropagation()}>
            <Show when={!showDeleteConfirm()} fallback={
              <div class="delete-confirm">
                <h3>Delete Contact?</h3>
                <p>Are you sure you want to delete {selectedContact()?.display_name}?</p>
                <p class="warning">This action cannot be undone.</p>
                <div class="dialog-actions">
                  <button class="danger" onClick={handleDelete}>Delete</button>
                  <button class="secondary" onClick={() => setShowDeleteConfirm(false)}>Cancel</button>
                </div>
              </div>
            }>
              <div class="contact-header">
                <div class="contact-avatar large">
                  {selectedContact()?.display_name.charAt(0).toUpperCase()}
                </div>
                <h3>{selectedContact()?.display_name}</h3>
                <span class={selectedContact()?.verified ? 'verified' : 'not-verified'}>
                  {selectedContact()?.verified ? '✓ Verified' : 'Not verified'}
                </span>
              </div>

              <Show when={error()}>
                <p class="error">{error()}</p>
              </Show>

              <div class="contact-fields">
                <Show when={selectedContact()?.fields.length === 0}>
                  <p class="empty-fields">No contact information shared yet.</p>
                </Show>
                <For each={selectedContact()?.fields}>
                  {(field) => (
                    <div class="field-item">
                      <span class="field-label">{field.label}</span>
                      <span class="field-value">{field.value}</span>
                      <span class="field-type">{field.field_type}</span>
                    </div>
                  )}
                </For>
              </div>

              <div class="contact-id">
                <span class="label">Contact ID</span>
                <span class="mono">{selectedContact()?.id.substring(0, 16)}...</span>
              </div>

              {/* Visibility Section */}
              <div class="visibility-section">
                <Show when={!showVisibility()} fallback={
                  <div class="visibility-list">
                    <h4>What {selectedContact()?.display_name} can see:</h4>
                    <Show when={visibilityRules().length === 0}>
                      <p class="empty-fields">You haven't added any fields to your card yet.</p>
                    </Show>
                    <For each={visibilityRules()}>
                      {(field) => (
                        <div class="visibility-item">
                          <span class="field-label">{field.field_label}</span>
                          <button
                            class={field.can_see ? 'visible' : 'hidden'}
                            onClick={() => toggleFieldVisibility(field.field_id, field.can_see)}
                          >
                            {field.can_see ? 'Visible' : 'Hidden'}
                          </button>
                        </div>
                      )}
                    </For>
                    <button class="secondary small" onClick={() => setShowVisibility(false)}>
                      Hide visibility settings
                    </button>
                  </div>
                }>
                  <button class="secondary" onClick={() => loadVisibilityRules(selectedContact()!.id)}>
                    Manage what they see
                  </button>
                </Show>
              </div>

              <div class="dialog-actions">
                <button class="secondary" onClick={closeDetail}>Close</button>
                <button class="danger" onClick={() => setShowDeleteConfirm(true)}>Delete Contact</button>
              </div>
            </Show>
          </div>
        </div>
      </Show>

      <nav class="bottom-nav">
        <button class="nav-btn" onClick={() => props.onNavigate('home')}>Home</button>
        <button class="nav-btn active">Contacts</button>
        <button class="nav-btn" onClick={() => props.onNavigate('exchange')}>Exchange</button>
        <button class="nav-btn" onClick={() => props.onNavigate('settings')}>Settings</button>
      </nav>
    </div>
  )
}

export default Contacts
