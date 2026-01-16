import { createResource, For, createSignal } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface FieldInfo {
  id: string
  field_type: string
  label: string
  value: string
}

interface CardInfo {
  display_name: string
  fields: FieldInfo[]
}

interface IdentityInfo {
  display_name: string
  public_id: string
}

interface HomeProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings') => void
}

async function fetchCard(): Promise<CardInfo> {
  return await invoke('get_card')
}

async function fetchIdentity(): Promise<IdentityInfo> {
  return await invoke('get_identity_info')
}

function Home(props: HomeProps) {
  const [card, { refetch: refetchCard }] = createResource(fetchCard)
  const [identity] = createResource(fetchIdentity)
  const [showAddField, setShowAddField] = createSignal(false)

  const fieldIcon = (type: string) => {
    switch (type.toLowerCase()) {
      case 'email': return 'mail'
      case 'phone': return 'phone'
      case 'website': return 'web'
      case 'address': return 'home'
      case 'social': return 'share'
      default: return 'note'
    }
  }

  return (
    <div class="page home">
      <header>
        <h1>Hello, {card()?.display_name || 'User'}!</h1>
        <p class="public-id">ID: {identity()?.public_id.substring(0, 16)}...</p>
      </header>

      <section class="card-section">
        <div class="section-header">
          <h2>Your Card</h2>
          <button class="icon-btn" onClick={() => setShowAddField(true)}>+ Add Field</button>
        </div>

        <div class="fields-list">
          <For each={card()?.fields}>
            {(field) => (
              <div class="field-item">
                <span class="field-icon">{fieldIcon(field.field_type)}</span>
                <div class="field-content">
                  <span class="field-label">{field.label}</span>
                  <span class="field-value">{field.value}</span>
                </div>
              </div>
            )}
          </For>

          {card()?.fields.length === 0 && (
            <p class="empty-state">No fields yet. Add your first field!</p>
          )}
        </div>
      </section>

      <nav class="bottom-nav">
        <button class="nav-btn active">Home</button>
        <button class="nav-btn" onClick={() => props.onNavigate('contacts')}>Contacts</button>
        <button class="nav-btn" onClick={() => props.onNavigate('exchange')}>Exchange</button>
        <button class="nav-btn" onClick={() => props.onNavigate('settings')}>Settings</button>
      </nav>

      {showAddField() && (
        <AddFieldDialog onClose={() => setShowAddField(false)} onAdd={() => { refetchCard(); setShowAddField(false) }} />
      )}
    </div>
  )
}

interface AddFieldDialogProps {
  onClose: () => void
  onAdd: () => void
}

function AddFieldDialog(props: AddFieldDialogProps) {
  const [fieldType, setFieldType] = createSignal('email')
  const [label, setLabel] = createSignal('')
  const [value, setValue] = createSignal('')
  const [error, setError] = createSignal('')

  const handleAdd = async () => {
    if (!label().trim() || !value().trim()) {
      setError('Please fill in all fields')
      return
    }

    try {
      await invoke('add_field', {
        fieldType: fieldType(),
        label: label(),
        value: value()
      })
      props.onAdd()
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div class="dialog-overlay" onClick={props.onClose}>
      <div class="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>Add Field</h3>

        <div class="form">
          <label>Type</label>
          <select value={fieldType()} onChange={(e) => setFieldType(e.target.value)}>
            <option value="email">Email</option>
            <option value="phone">Phone</option>
            <option value="website">Website</option>
            <option value="address">Address</option>
            <option value="social">Social</option>
            <option value="custom">Custom</option>
          </select>

          <label>Label</label>
          <input
            type="text"
            placeholder="e.g., Work, Personal"
            value={label()}
            onInput={(e) => setLabel(e.target.value)}
          />

          <label>Value</label>
          <input
            type="text"
            placeholder="Enter value"
            value={value()}
            onInput={(e) => setValue(e.target.value)}
          />

          {error() && <p class="error">{error()}</p>}

          <div class="dialog-actions">
            <button class="secondary" onClick={props.onClose}>Cancel</button>
            <button onClick={handleAdd}>Add</button>
          </div>
        </div>
      </div>
    </div>
  )
}

export default Home
