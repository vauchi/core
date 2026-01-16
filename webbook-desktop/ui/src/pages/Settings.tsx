import { createResource } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface IdentityInfo {
  display_name: string
  public_id: string
}

interface SettingsProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings') => void
}

async function fetchIdentity(): Promise<IdentityInfo> {
  return await invoke('get_identity_info')
}

function Settings(props: SettingsProps) {
  const [identity] = createResource(fetchIdentity)

  return (
    <div class="page settings">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Settings</h1>
      </header>

      <section class="settings-section">
        <h2>Identity</h2>
        <div class="setting-item">
          <span class="setting-label">Display Name</span>
          <span class="setting-value">{identity()?.display_name}</span>
        </div>
        <div class="setting-item">
          <span class="setting-label">Public ID</span>
          <span class="setting-value mono">{identity()?.public_id}</span>
        </div>
      </section>

      <section class="settings-section">
        <h2>Backup</h2>
        <p class="setting-description">
          Export your identity to back it up or transfer to another device.
        </p>
        <button class="secondary">Export Backup</button>
      </section>

      <section class="settings-section">
        <h2>About</h2>
        <div class="setting-item">
          <span class="setting-label">Version</span>
          <span class="setting-value">0.1.0</span>
        </div>
        <div class="setting-item">
          <span class="setting-label">WebBook</span>
          <span class="setting-value">Privacy-focused contact card exchange</span>
        </div>
      </section>

      <nav class="bottom-nav">
        <button class="nav-btn" onClick={() => props.onNavigate('home')}>Home</button>
        <button class="nav-btn" onClick={() => props.onNavigate('contacts')}>Contacts</button>
        <button class="nav-btn" onClick={() => props.onNavigate('exchange')}>Exchange</button>
        <button class="nav-btn active">Settings</button>
      </nav>
    </div>
  )
}

export default Settings
