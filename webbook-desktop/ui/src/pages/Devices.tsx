import { createResource, createSignal, For, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface DeviceInfo {
  device_id: string
  device_name: string
  device_index: number
  is_current: boolean
  is_active: boolean
}

interface JoinDeviceResult {
  success: boolean
  device_name: string
  message: string
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
  const [showJoinDialog, setShowJoinDialog] = createSignal(false)
  const [showRevokeConfirm, setShowRevokeConfirm] = createSignal<DeviceInfo | null>(null)
  const [linkData, setLinkData] = createSignal('')
  const [joinData, setJoinData] = createSignal('')
  const [error, setError] = createSignal('')
  const [joinMessage, setJoinMessage] = createSignal('')
  const [isJoining, setIsJoining] = createSignal(false)
  const [isRevoking, setIsRevoking] = createSignal(false)

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

  const handleJoinDevice = async () => {
    if (!joinData().trim()) {
      setJoinMessage('Please paste the device link data')
      return
    }

    setIsJoining(true)
    setJoinMessage('')

    try {
      const result = await invoke('join_device', { linkData: joinData() }) as JoinDeviceResult
      setJoinMessage(result.message)
      if (result.success) {
        refetch()
        setTimeout(() => {
          setShowJoinDialog(false)
          setJoinData('')
          setJoinMessage('')
        }, 2000)
      }
    } catch (e) {
      setJoinMessage(String(e))
    }

    setIsJoining(false)
  }

  const handleRevokeDevice = async (device: DeviceInfo) => {
    setIsRevoking(true)
    setError('')

    try {
      await invoke('revoke_device', { deviceId: device.device_id })
      refetch()
      setShowRevokeConfirm(null)
    } catch (e) {
      setError(String(e))
    }

    setIsRevoking(false)
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
          <div class="header-buttons">
            <button class="small secondary" onClick={() => setShowJoinDialog(true)}>Join Another</button>
            <button class="small primary" onClick={generateLink}>+ Link Device</button>
          </div>
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
                <Show when={!device.is_current && device.is_active}>
                  <button
                    class="small danger"
                    onClick={() => setShowRevokeConfirm(device)}
                  >
                    Revoke
                  </button>
                </Show>
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

      {/* Join Device Dialog */}
      <Show when={showJoinDialog()}>
        <div class="dialog-overlay" onClick={() => {
          if (!isJoining()) {
            setShowJoinDialog(false)
            setJoinData('')
            setJoinMessage('')
          }
        }}>
          <div class="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Join Another Device</h3>
            <p>Paste the device link data from your other device:</p>

            <textarea
              value={joinData()}
              onInput={(e) => setJoinData(e.target.value)}
              placeholder="Paste device link data here..."
              rows={4}
              disabled={isJoining()}
            />

            <Show when={joinMessage()}>
              <p class="info-message">{joinMessage()}</p>
            </Show>

            <div class="dialog-actions">
              <button
                class="primary"
                onClick={handleJoinDevice}
                disabled={isJoining() || !joinData().trim()}
              >
                {isJoining() ? 'Joining...' : 'Join'}
              </button>
              <button
                class="secondary"
                onClick={() => {
                  setShowJoinDialog(false)
                  setJoinData('')
                  setJoinMessage('')
                }}
                disabled={isJoining()}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Revoke Confirmation Dialog */}
      <Show when={showRevokeConfirm()}>
        <div class="dialog-overlay" onClick={() => {
          if (!isRevoking()) setShowRevokeConfirm(null)
        }}>
          <div class="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Revoke Device</h3>
            <p>Are you sure you want to revoke <strong>{showRevokeConfirm()?.device_name}</strong>?</p>
            <p class="warning">This device will no longer be able to sync with your account.</p>

            <div class="dialog-actions">
              <button
                class="danger"
                onClick={() => handleRevokeDevice(showRevokeConfirm()!)}
                disabled={isRevoking()}
              >
                {isRevoking() ? 'Revoking...' : 'Revoke Device'}
              </button>
              <button
                class="secondary"
                onClick={() => setShowRevokeConfirm(null)}
                disabled={isRevoking()}
              >
                Cancel
              </button>
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
