import { createResource, createSignal, Show, createEffect, onMount } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'
import QRCode from 'qrcode'

interface ExchangeQRResponse {
  data: string
  display_name: string
  qr_ascii: string
}

interface ExchangeResult {
  success: boolean
  contact_name: string
  contact_id: string
  message: string
}

interface ExchangeProps {
  onNavigate: (page: 'home' | 'contacts' | 'exchange' | 'settings' | 'devices' | 'recovery') => void
}

async function generateQR(): Promise<ExchangeQRResponse> {
  return await invoke('generate_qr')
}

function Exchange(props: ExchangeProps) {
  const [qrData] = createResource(generateQR)
  const [scanData, setScanData] = createSignal('')
  const [result, setResult] = createSignal<ExchangeResult | null>(null)
  const [error, setError] = createSignal('')
  const [qrImageUrl, setQrImageUrl] = createSignal('')

  // Generate QR code image when data is available
  createEffect(async () => {
    const data = qrData()
    if (data?.data) {
      try {
        const url = await QRCode.toDataURL(data.data, {
          width: 256,
          margin: 2,
          color: {
            dark: '#000000',
            light: '#ffffff'
          }
        })
        setQrImageUrl(url)
      } catch (e) {
        console.error('Failed to generate QR image:', e)
      }
    }
  })

  const handleComplete = async () => {
    if (!scanData().trim()) {
      setError('Please enter the exchange data')
      return
    }

    try {
      const exchangeResult = await invoke('complete_exchange', { data: scanData() }) as ExchangeResult
      setResult(exchangeResult)
      setError('')
      setScanData('')
    } catch (e) {
      setError(String(e))
      setResult(null)
    }
  }

  const copyToClipboard = async () => {
    const data = qrData()?.data
    if (data) {
      await navigator.clipboard.writeText(data)
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
            <Show when={qrImageUrl()} fallback={
              <pre class="qr-ascii">{qrData()?.qr_ascii}</pre>
            }>
              <img src={qrImageUrl()} alt="Exchange QR Code" class="qr-image" />
            </Show>
            <p class="display-name">{qrData()?.display_name}</p>
          </div>
        </Show>

        <div class="copy-section">
          <p>Or share this data:</p>
          <div class="copy-input-group">
            <input type="text" readonly value={qrData()?.data || ''} />
            <button class="copy-btn" onClick={copyToClipboard}>Copy</button>
          </div>
        </div>
      </section>

      <section class="scan-section">
        <h2>Complete Exchange</h2>
        <p>Paste the exchange data from another user</p>

        <input
          type="text"
          placeholder="Paste exchange data here..."
          value={scanData()}
          onInput={(e) => setScanData(e.target.value)}
        />

        <Show when={error()}>
          <p class="error">{error()}</p>
        </Show>

        <Show when={result()}>
          <div class={result()?.success ? 'success' : 'warning'}>
            <p>{result()?.message}</p>
            <Show when={result()?.success}>
              <p>Added: {result()?.contact_name}</p>
            </Show>
          </div>
        </Show>

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
