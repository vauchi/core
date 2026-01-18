import { createSignal } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'

interface SetupProps {
  onComplete: () => void
}

function Setup(props: SetupProps) {
  const [name, setName] = createSignal('')
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal('')

  const handleCreate = async () => {
    if (!name().trim()) {
      setError('Please enter your name')
      return
    }

    setLoading(true)
    setError('')

    try {
      await invoke('create_identity', { name: name() })
      props.onComplete()
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div class="page setup">
      <div class="setup-container">
        <h1>Welcome to Vauchi</h1>
        <p>Privacy-focused contact card exchange</p>

        <div class="form">
          <label for="name">Your Display Name</label>
          <input
            id="name"
            type="text"
            placeholder="Enter your name"
            value={name()}
            onInput={(e) => setName(e.target.value)}
            disabled={loading()}
          />

          {error() && <p class="error">{error()}</p>}

          <button onClick={handleCreate} disabled={loading()}>
            {loading() ? 'Creating...' : 'Get Started'}
          </button>
        </div>
      </div>
    </div>
  )
}

export default Setup
