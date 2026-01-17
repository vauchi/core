import { createSignal, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface RecoverySettingsInfo {
  recovery_threshold: number
  verification_threshold: number
}

interface ClaimInfo {
  old_pk: string
  new_pk: string
  is_expired: boolean
  contact_name: string | null
}

interface RecoveryProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings' | 'devices' | 'recovery') => void
}

function Recovery(props: RecoveryProps) {
  const [mode, setMode] = createSignal<'menu' | 'claim' | 'vouch'>('menu')
  const [error, setError] = createSignal('')
  const [success, setSuccess] = createSignal('')

  // Claim state
  const [oldPkHex, setOldPkHex] = createSignal('')
  const [claimData, setClaimData] = createSignal('')

  // Vouch state
  const [vouchInput, setVouchInput] = createSignal('')
  const [claimInfo, setClaimInfo] = createSignal<ClaimInfo | null>(null)
  const [voucherData, setVoucherData] = createSignal('')

  const createClaim = async () => {
    if (!oldPkHex().trim()) {
      setError('Please enter your old public key')
      return
    }

    try {
      const claim = await invoke('create_recovery_claim', { oldPkHex: oldPkHex() }) as string
      setClaimData(claim)
      setError('')
      setSuccess('Recovery claim created!')
    } catch (e) {
      setError(String(e))
    }
  }

  const parseClaim = async () => {
    if (!vouchInput().trim()) {
      setError('Please enter a recovery claim')
      return
    }

    try {
      const info = await invoke('parse_recovery_claim', { claimB64: vouchInput() }) as ClaimInfo
      setClaimInfo(info)
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }

  const createVoucher = async () => {
    try {
      const voucher = await invoke('create_recovery_voucher', { claimB64: vouchInput() }) as string
      setVoucherData(voucher)
      setError('')
      setSuccess('Voucher created!')
    } catch (e) {
      setError(String(e))
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text)
  }

  return (
    <div class="page recovery">
      <header>
        <button class="back-btn" onClick={() => {
          if (mode() === 'menu') {
            props.onNavigate('home')
          } else {
            setMode('menu')
            setError('')
            setSuccess('')
            setClaimData('')
            setVoucherData('')
            setClaimInfo(null)
          }
        }}>← Back</button>
        <h1>Recovery</h1>
      </header>

      <Show when={error()}>
        <p class="error">{error()}</p>
      </Show>

      <Show when={success()}>
        <p class="success">{success()}</p>
      </Show>

      {/* Menu Mode */}
      <Show when={mode() === 'menu'}>
        <section class="recovery-menu">
          <div class="menu-item" onClick={() => setMode('claim')}>
            <div class="menu-icon">🔑</div>
            <div class="menu-content">
              <h3>Create Recovery Claim</h3>
              <p>Lost your device? Start the recovery process.</p>
            </div>
          </div>

          <div class="menu-item" onClick={() => setMode('vouch')}>
            <div class="menu-icon">✅</div>
            <div class="menu-content">
              <h3>Vouch for Contact</h3>
              <p>Help a contact recover their identity.</p>
            </div>
          </div>
        </section>

        <section class="info-section">
          <h3>How Recovery Works</h3>
          <ol>
            <li>Create a new identity on a new device</li>
            <li>Generate a recovery claim with your OLD public key</li>
            <li>Meet with 3+ trusted contacts in person</li>
            <li>Have them vouch for your recovery</li>
            <li>Collect vouchers and share your recovery proof</li>
          </ol>
        </section>
      </Show>

      {/* Create Claim Mode */}
      <Show when={mode() === 'claim'}>
        <section class="recovery-form">
          <h2>Create Recovery Claim</h2>
          <p>Enter the public key from your lost identity:</p>

          <div class="form">
            <label>Old Public Key (hex)</label>
            <input
              type="text"
              placeholder="Enter 64-character hex string"
              value={oldPkHex()}
              onInput={(e) => setOldPkHex(e.target.value)}
            />

            <button onClick={createClaim}>Generate Claim</button>
          </div>

          <Show when={claimData()}>
            <div class="result-box">
              <h3>Your Recovery Claim</h3>
              <p>Share this with your contacts:</p>
              <code class="claim-data">{claimData()}</code>
              <button class="small" onClick={() => copyToClipboard(claimData())}>Copy</button>
            </div>
          </Show>
        </section>
      </Show>

      {/* Vouch Mode */}
      <Show when={mode() === 'vouch'}>
        <section class="recovery-form">
          <h2>Vouch for Contact</h2>
          <p>Paste the recovery claim from your contact:</p>

          <div class="form">
            <label>Recovery Claim</label>
            <textarea
              placeholder="Paste claim data here"
              value={vouchInput()}
              onInput={(e) => setVouchInput(e.target.value)}
              rows={4}
            />

            <button onClick={parseClaim}>Verify Claim</button>
          </div>

          <Show when={claimInfo()}>
            <div class="claim-preview">
              <h3>Claim Details</h3>
              <p><strong>Old Identity:</strong> {claimInfo()?.old_pk.substring(0, 16)}...</p>
              <p><strong>New Identity:</strong> {claimInfo()?.new_pk.substring(0, 16)}...</p>

              <Show when={claimInfo()?.contact_name}>
                <p class="success">Matches your contact: {claimInfo()?.contact_name}</p>
              </Show>

              <Show when={!claimInfo()?.contact_name}>
                <p class="warning">This key is NOT in your contacts. Verify in person!</p>
              </Show>

              <Show when={claimInfo()?.is_expired}>
                <p class="error">This claim has expired!</p>
              </Show>

              <Show when={!claimInfo()?.is_expired}>
                <button class="primary" onClick={createVoucher}>Create Voucher</button>
              </Show>
            </div>
          </Show>

          <Show when={voucherData()}>
            <div class="result-box">
              <h3>Your Voucher</h3>
              <p>Give this to the person recovering:</p>
              <code class="voucher-data">{voucherData()}</code>
              <button class="small" onClick={() => copyToClipboard(voucherData())}>Copy</button>
            </div>
          </Show>
        </section>
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

export default Recovery
