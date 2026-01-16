import { createResource, createSignal, Show } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface ExchangeQR {
  data: string
  display_name: string
}

interface ExchangeProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings') => void
}

async function generateQR(): Promise<ExchangeQR> {
  return await invoke('generate_qr')
}

function Exchange(props: ExchangeProps) {
  const [qrData] = createResource(generateQR)
  const [scanData, setScanData] = createSignal('')
  const [result, setResult] = createSignal('')
  const [error, setError] = createSignal('')

  const handleComplete = async () => {
    if (!scanData().trim()) {
      setError('Please enter the exchange data')
      return
    }

    try {
      const result = await invoke('complete_exchange', { data: scanData() }) as string
      setResult(result)
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div class="page exchange">
      <header>
        <button class="back-btn" onClick={() => props.onNavigate('home')}>← Back</button>
        <h1>Exchange</h1>
      </header>

      <section class="qr-section">
        <h2>Your QR Code</h2>
        <p>Have someone scan this to add you as a contact</p>

        <Show when={qrData()} fallback={<div class="loading">Generating QR...</div>}>
          <div class="qr-container">
            {/* Simple QR placeholder - in production use qrcode library */}
            <div class="qr-placeholder">
              <p>📱</p>
              <p class="qr-data">{qrData()?.data.substring(0, 30)}...</p>
            </div>
          </div>
        </Show>

        <div class="copy-section">
          <p>Or share this link:</p>
          <input type="text" readonly value={qrData()?.data || ''} />
        </div>
      </section>

      <section class="scan-section">
        <h2>Complete Exchange</h2>
        <p>Paste the exchange data from another user</p>

        <input
          type="text"
          placeholder="wb://..."
          value={scanData()}
          onInput={(e) => setScanData(e.target.value)}
        />

        {error() && <p class="error">{error()}</p>}
        {result() && <p class="success">{result()}</p>}

        <button onClick={handleComplete}>Complete Exchange</button>
      </section>

      <nav class="bottom-nav">
        <button class="nav-btn" onClick={() => props.onNavigate('home')}>Home</button>
        <button class="nav-btn" onClick={() => props.onNavigate('contacts')}>Contacts</button>
        <button class="nav-btn active">Exchange</button>
        <button class="nav-btn" onClick={() => props.onNavigate('settings')}>Settings</button>
      </nav>
    </div>
  )
}

export default Exchange
