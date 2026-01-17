import { createResource, createSignal, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface IdentityInfo {
  display_name: string
  public_id: string
}

interface BackupResult {
  success: boolean
  data: string | null
  error: string | null
}

interface SettingsProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings') => void
}

async function fetchIdentity(): Promise<IdentityInfo> {
  return await invoke('get_identity_info')
}

function Settings(props: SettingsProps) {
  const [identity] = createResource(fetchIdentity)
  const [showBackupDialog, setShowBackupDialog] = createSignal(false)
  const [backupPassword, setBackupPassword] = createSignal('')
  const [confirmPassword, setConfirmPassword] = createSignal('')
  const [backupData, setBackupData] = createSignal('')
  const [backupError, setBackupError] = createSignal('')
  const [passwordStrength, setPasswordStrength] = createSignal('')

  const checkPassword = async () => {
    const password = backupPassword()
    if (password.length < 8) {
      setPasswordStrength('')
      return
    }
    try {
      const strength = await invoke('check_password_strength', { password }) as string
      setPasswordStrength(strength)
    } catch (e) {
      setPasswordStrength('')
    }
  }

  const handleExportBackup = async () => {
    setBackupError('')

    if (backupPassword() !== confirmPassword()) {
      setBackupError('Passwords do not match')
      return
    }

    if (backupPassword().length < 8) {
      setBackupError('Password must be at least 8 characters')
      return
    }

    try {
      // Check password strength
      await invoke('check_password_strength', { password: backupPassword() })

      // Export backup
      const result = await invoke('export_backup', { password: backupPassword() }) as BackupResult

      if (result.success && result.data) {
        setBackupData(result.data)
        setBackupError('')
      } else {
        setBackupError(result.error || 'Export failed')
      }
    } catch (e) {
      setBackupError(String(e))
    }
  }

  const copyBackup = async () => {
    await navigator.clipboard.writeText(backupData())
  }

  const closeDialog = () => {
    setShowBackupDialog(false)
    setBackupPassword('')
    setConfirmPassword('')
    setBackupData('')
    setBackupError('')
    setPasswordStrength('')
  }

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
        <button class="secondary" onClick={() => setShowBackupDialog(true)}>Export Backup</button>
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

      {/* Backup Dialog */}
      <Show when={showBackupDialog()}>
        <div class="dialog-overlay" onClick={closeDialog}>
          <div class="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Export Backup</h3>

            <Show when={!backupData()} fallback={
              <div class="backup-result">
                <p class="success">Backup created successfully!</p>
                <textarea readonly value={backupData()} rows={6} />
                <div class="dialog-actions">
                  <button class="primary" onClick={copyBackup}>Copy to Clipboard</button>
                  <button class="secondary" onClick={closeDialog}>Close</button>
                </div>
              </div>
            }>
              <div class="backup-form">
                <p>Enter a strong password to encrypt your backup.</p>

                <label>Password</label>
                <input
                  type="password"
                  value={backupPassword()}
                  onInput={(e) => {
                    setBackupPassword(e.target.value)
                    checkPassword()
                  }}
                  placeholder="Enter password"
                />
                <Show when={passwordStrength()}>
                  <p class="password-strength">Strength: {passwordStrength()}</p>
                </Show>

                <label>Confirm Password</label>
                <input
                  type="password"
                  value={confirmPassword()}
                  onInput={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Confirm password"
                />

                <Show when={backupError()}>
                  <p class="error">{backupError()}</p>
                </Show>

                <div class="dialog-actions">
                  <button class="primary" onClick={handleExportBackup}>Export</button>
                  <button class="secondary" onClick={closeDialog}>Cancel</button>
                </div>
              </div>
            </Show>
          </div>
        </div>
      </Show>

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
