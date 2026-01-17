import { createResource, createSignal, For, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface DeviceInfo {
  device_id: string
  device_name: string
  device_index: number
  is_current: boolean
  is_active: boolean
}

interface DevicesProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings' | 'devices' | 'recovery') => void
}

async function fetchDevices(): Promise<DeviceInfo[]> {
  return await invoke('list_devices')
}

function Devices(props: DevicesProps) {
  const [devices, { refetch }] = createResource(fetchDevices)
  const [showLinkDialog, setShowLinkDialog] = createSignal(false)
  const [linkData, setLinkData] = createSignal('')
  const [error, setError] = createSignal('')

  const generateLink = async () => {
    try {
      const data = await invoke('generate_device_link') as string
      setLinkData(data)
      setShowLinkDialog(true)
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }

  const copyLinkData = () => {
    navigator.clipboard.writeText(linkData())
  }

  return (
    <div class="page devices">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Devices</h1>
      </header>

      <Show when={error()}>
        <p class="error">{error()}</p>
      </Show>

      <section class="devices-section">
        <div class="section-header">
          <h2>Linked Devices</h2>
          <button class="icon-btn" onClick={generateLink}>+ Link Device</button>
        </div>

        <div class="devices-list">
          <For each={devices()}>
            {(device) => (
              <div class={`device-item ${device.is_current ? 'current' : ''}`}>
                <div class="device-icon">
                  {device.is_current ? '📱' : '💻'}
                </div>
                <div class="device-info">
                  <span class="device-name">
                    {device.device_name}
                    {device.is_current && <span class="badge current">This device</span>}
                  </span>
                  <span class="device-id">ID: {device.device_id.substring(0, 16)}...</span>
                  <span class={`device-status ${device.is_active ? 'active' : 'revoked'}`}>
                    {device.is_active ? 'Active' : 'Revoked'}
                  </span>
                </div>
              </div>
            )}
          </For>

          {devices()?.length === 0 && (
            <p class="empty-state">No devices found</p>
          )}
        </div>
      </section>

      <section class="info-section">
        <h3>Multi-Device Sync</h3>
        <p>Link multiple devices to access your contacts from anywhere.</p>
        <p>All devices share the same identity and stay in sync.</p>
      </section>

      {/* Link Device Dialog */}
      <Show when={showLinkDialog()}>
        <div class="dialog-overlay" onClick={() => setShowLinkDialog(false)}>
          <div class="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Link New Device</h3>
            <p>Scan this code with your new device, or copy the data below:</p>

            <div class="link-data">
              <code>{linkData().substring(0, 50)}...</code>
              <button class="small" onClick={copyLinkData}>Copy</button>
            </div>

            <p class="warning">This code expires in 10 minutes.</p>

            <div class="dialog-actions">
              <button class="secondary" onClick={() => setShowLinkDialog(false)}>Close</button>
            </div>
          </div>
        </div>
      </Show>

      <nav class="bottom-nav">
        <button class="nav-btn" onClick={() => props.onNavigate('home')}>Home</button>
        <button class="nav-btn" onClick={() => props.onNavigate('contacts')}>Contacts</button>
        <button class="nav-btn" onClick={() => props.onNavigate('exchange')}>Exchange</button>
        <button class="nav-btn" onClick={() => props.onNavigate('settings')}>Settings</button>
      </nav>
    </div>
  )
}

export default Devices
