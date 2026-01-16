import { createResource, For } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface ContactInfo {
  id: string
  display_name: string
  verified: boolean
}

interface ContactsProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings') => void
}

async function fetchContacts(): Promise<ContactInfo[]> {
  return await invoke('list_contacts')
}

function Contacts(props: ContactsProps) {
  const [contacts] = createResource(fetchContacts)

  return (
    <div class="page contacts">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Contacts</h1>
      </header>

      <div class="contacts-list">
        <For each={contacts()}>
          {(contact) => (
            <div class="contact-item">
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
