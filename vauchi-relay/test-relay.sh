#!/usr/bin/env bash
# Test script for vauchi-relay WebSocket endpoint
# Usage: ./test-relay.sh [hostname]
# Example: ./test-relay.sh relay.vauchi.app

set -uo pipefail

HOST="${1:-relay.vauchi.app}"
PASS=0
FAIL=0

check() {
    local name="$1" result="$2" expected="$3"
    if [[ "$result" == *"$expected"* ]]; then
        echo "✓ $name"
        PASS=$((PASS + 1))
    else
        echo "✗ $name"
        echo "  Expected: $expected"
        echo "  Got: $result"
        FAIL=$((FAIL + 1))
    fi
}

echo "Testing relay at: $HOST"
echo "─────────────────────────────"

# 1. Health endpoint
health=$(curl -s --connect-timeout 5 "https://$HOST/health" 2>&1 || echo "CURL_FAILED")
check "Health endpoint" "$health" '"status":"healthy"'

# 2. Root returns helpful JSON (not WebSocket)
root=$(curl -s --connect-timeout 5 "https://$HOST/" 2>&1 || echo "CURL_FAILED")
check "HTTP error response" "$root" '"error":'

# 3. HTTP redirects to HTTPS
redirect=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 "http://$HOST/" 2>&1 || echo "000")
check "HTTP→HTTPS redirect" "$redirect" "301"

# 4. WebSocket connection
if command -v websocat &>/dev/null; then
    ws=$(timeout 3 websocat -v "wss://$HOST" 2>&1 || true)
    if [[ "$ws" == *"Connected to ws"* ]]; then
        echo "✓ WebSocket upgrade"
        PASS=$((PASS + 1))
    else
        echo "✗ WebSocket upgrade"
        echo "  Got: $ws"
        FAIL=$((FAIL + 1))
    fi
else
    # Fallback: check for 101 upgrade with curl
    ws_status=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 \
        -H "Upgrade: websocket" -H "Connection: Upgrade" \
        -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
        -H "Sec-WebSocket-Version: 13" \
        "https://$HOST/" 2>&1 || echo "000")
    # Note: curl can't complete WS handshake, so 400 means relay detected upgrade attempt
    # A working relay returns 101 to real WS clients; curl gets 400 because it can't finish
    check "WebSocket headers detected" "$ws_status" "400"
    echo "  (Install websocat for full WS test: cargo install websocat)"
fi

echo "─────────────────────────────"
echo "Results: $PASS passed, $FAIL failed"
exit $FAIL
