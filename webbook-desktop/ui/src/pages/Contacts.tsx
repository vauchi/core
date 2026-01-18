import { createResource, createSignal, createMemo, For, Show } from 'solid-js'
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

interface OpenResult {
  success: boolean
  action: string
  uri: string | null
  error: string | null
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

interface FingerprintInfo {
  their_fingerprint: string
  our_fingerprint: string
  formatted_their: string
  formatted_our: string
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
  const [showVerification, setShowVerification] = createSignal(false)
  const [visibilityRules, setVisibilityRules] = createSignal<FieldVisibilityInfo[]>([])
  const [fingerprint, setFingerprint] = createSignal<FingerprintInfo | null>(null)
  const [isVerifying, setIsVerifying] = createSignal(false)
  const [error, setError] = createSignal('')
  const [searchQuery, setSearchQuery] = createSignal('')

  // Filter contacts based on search query
  const filteredContacts = createMemo(() => {
    const query = searchQuery().toLowerCase().trim()
    if (!query) return contacts() || []

    return (contacts() || []).filter(c =>
      c.display_name.toLowerCase().includes(query)
    )
  })

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
    setShowVerification(false)
    setVisibilityRules([])
    setFingerprint(null)
    setError('')
  }

  const loadFingerprint = async (contactId: string) => {
    try {
      const fp = await invoke('get_contact_fingerprint', { id: contactId }) as FingerprintInfo
      setFingerprint(fp)
      setShowVerification(true)
    } catch (e) {
      setError(String(e))
    }
  }

  const handleVerifyContact = async () => {
    const contact = selectedContact()
    if (!contact) return

    setIsVerifying(true)
    try {
      await invoke('verify_contact', { id: contact.id })
      // Refresh the contact details
      const details = await invoke('get_contact', { id: contact.id }) as ContactDetails
      setSelectedContact(details)
      setShowVerification(false)
      setFingerprint(null)
      refetch()
    } catch (e) {
      setError(String(e))
    }
    setIsVerifying(false)
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

  const handleFieldClick = async (field: FieldInfo) => {
    try {
      const result = await invoke('open_contact_field', {
        fieldType: field.field_type,
        label: field.label,
        value: field.value
      }) as OpenResult

      if (!result.success && result.error) {
        // If opening failed, copy to clipboard as fallback
        await navigator.clipboard.writeText(field.value)
        setError(`${result.error} Value copied to clipboard.`)
      }
    } catch (e) {
      // Fallback: copy to clipboard
      await navigator.clipboard.writeText(field.value)
      setError(`Could not open. Copied to clipboard.`)
    }
  }

  const getFieldIcon = (fieldType: string): string => {
    switch (fieldType.toLowerCase()) {
      case 'phone': return '📞'
      case 'email': return '✉️'
      case 'website': return '🌐'
      case 'address': return '📍'
      case 'social': return '👤'
      default: return '📋'
    }
  }

  return (
    <div class="page contacts">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Contacts</h1>
      </header>

      <div class="search-bar">
        <input
          type="text"
          placeholder="Search contacts..."
          value={searchQuery()}
          onInput={(e) => setSearchQuery(e.target.value)}
        />
        <Show when={searchQuery()}>
          <button class="clear-search" onClick={() => setSearchQuery('')}>×</button>
        </Show>
      </div>

      <div class="contacts-list">
        <For each={filteredContacts()}>
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

        {filteredContacts().length === 0 && searchQuery() && (
          <div class="empty-state">
            <p>No contacts match "{searchQuery()}"</p>
            <button class="secondary" onClick={() => setSearchQuery('')}>Clear search</button>
          </div>
        )}

        {contacts()?.length === 0 && !searchQuery() && (
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
                <Show when={!selectedContact()?.verified}>
                  <button
                    class="small primary"
                    onClick={() => loadFingerprint(selectedContact()!.id)}
                  >
                    Verify Identity
                  </button>
                </Show>
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
                    <div
                      class="field-item clickable"
                      onClick={() => handleFieldClick(field)}
                      title={`Click to open ${field.field_type.toLowerCase()}`}
                    >
                      <span class="field-icon">{getFieldIcon(field.field_type)}</span>
                      <div class="field-content">
                        <span class="field-label">{field.label}</span>
                        <span class="field-value">{field.value}</span>
                      </div>
                      <span class="field-action">→</span>
                    </div>
                  )}
                </For>
              </div>

              <div class="contact-id">
                <span class="label">Contact ID</span>
                <span class="mono">{selectedContact()?.id.substring(0, 16)}...</span>
              </div>

              {/* Verification Section */}
              <Show when={showVerification() && fingerprint()}>
                <div class="verification-section">
                  <h4>Verify {selectedContact()?.display_name}'s Identity</h4>
                  <p class="verify-instructions">
                    Compare these fingerprints with {selectedContact()?.display_name} in person
                    to verify their identity.
                  </p>

                  <div class="fingerprint-comparison">
                    <div class="fingerprint-block">
                      <span class="fp-label">Their Fingerprint</span>
                      <code class="fingerprint">{fingerprint()?.formatted_their}</code>
                    </div>
                    <div class="fingerprint-block">
                      <span class="fp-label">Your Fingerprint</span>
                      <code class="fingerprint">{fingerprint()?.formatted_our}</code>
                    </div>
                  </div>

                  <p class="verify-warning">
                    Only mark as verified if you have confirmed these fingerprints match
                    the ones shown on their device.
                  </p>

                  <div class="verification-actions">
                    <button
                      class="primary"
                      onClick={handleVerifyContact}
                      disabled={isVerifying()}
                    >
                      {isVerifying() ? 'Verifying...' : 'Mark as Verified'}
                    </button>
                    <button
                      class="secondary"
                      onClick={() => {
                        setShowVerification(false)
                        setFingerprint(null)
                      }}
                      disabled={isVerifying()}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              </Show>

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
