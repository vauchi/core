# Plan: Relay Server Hardening

**Priority**: CRITICAL
**Effort**: 3-5 days
**Impact**: Security & Performance

## Problem Statement

The relay server has critical security and performance gaps that could cause service disruption or data exposure in production:

1. **Connection limit not enforced** - DoS via connection exhaustion
2. **No TLS support** - Data transmitted in plaintext
3. **SQLite bottleneck** - Single-threaded writes limit throughput
4. **Rate limiter memory leak** - Unbounded bucket map growth

## Success Criteria

- [x] Connection limit actually enforced (reject when at max)
- [ ] TLS termination documented or native support added (deferred - use reverse proxy)
- [x] SQLite WAL mode enabled, ~3x write throughput
- [x] Rate limiter buckets cleaned up after inactivity
- [x] All changes have tests (47 tests passing)

## Implementation

### Task 1: Enforce Connection Limit

**File**: `vauchi-relay/src/main.rs`

```rust
// Add to accept loop (around line 133)
let connections = Arc::new(AtomicUsize::new(0));

loop {
    let (stream, addr) = listener.accept().await?;

    let current = connections.load(Ordering::SeqCst);
    if current >= config.max_connections {
        warn!("Connection rejected: at max capacity ({}/{})", current, config.max_connections);
        metrics.increment_connections_rejected();
        drop(stream);
        continue;
    }

    connections.fetch_add(1, Ordering::SeqCst);
    // ... spawn task
    // In task cleanup: connections.fetch_sub(1, Ordering::SeqCst);
}
```

**Test**: Connect max_connections + 1 clients, verify last is rejected.

### Task 2: Add TLS Support (Option A: Native)

**Files**:
- `vauchi-relay/Cargo.toml` - Add `tokio-rustls`
- `vauchi-relay/src/config.rs` - Add TLS config fields
- `vauchi-relay/src/main.rs` - Wrap acceptor in TLS

```rust
// config.rs additions
pub tls_cert_path: Option<PathBuf>,
pub tls_key_path: Option<PathBuf>,
```

**Alternative (Option B)**: Document reverse proxy requirement clearly in README with nginx/caddy examples.

### Task 3: SQLite WAL Mode

**File**: `vauchi-relay/src/storage/sqlite.rs`

```rust
// In SqliteBlobStore::new() after connection open
conn.execute_batch("
    PRAGMA journal_mode=WAL;
    PRAGMA synchronous=NORMAL;
    PRAGMA cache_size=10000;
")?;
```

**Test**: Benchmark before/after with 1000 concurrent writes.

### Task 4: Rate Limiter Cleanup

**File**: `vauchi-relay/src/rate_limit.rs`

```rust
// Add last_access timestamp to TokenBucket
struct TokenBucket {
    tokens: f64,
    last_update: Instant,
    last_access: Instant,  // NEW
}

// Implement actual cleanup
pub fn cleanup_inactive(&self, max_idle: Duration) {
    let mut buckets = self.buckets.write().unwrap();
    let now = Instant::now();
    buckets.retain(|_, bucket| {
        now.duration_since(bucket.last_access) < max_idle
    });
}
```

**Call from main.rs cleanup loop** (every 10 minutes, idle > 30 minutes).

### Task 5: Metrics Recording

**File**: `vauchi-relay/src/handler.rs`

Actually increment the metrics counters that are defined but unused:
- `messages_received` on message receipt
- `messages_sent` on blob store
- `rate_limited` when rate limit hit

## Verification

```bash
# Run tests
cargo test -p vauchi-relay

# Load test (requires wrk or similar)
wrk -c 1000 -d 30s ws://localhost:8080

# Check metrics
curl localhost:8081/metrics | grep -E "connections|rate_limited"
```

## Rollback Plan

Each change is isolated. Revert specific commits if issues arise.

## Dependencies

- None (self-contained relay crate)
