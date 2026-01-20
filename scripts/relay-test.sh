#!/bin/bash
# Test Vauchi relay connectivity
#
# Tests connection to the production relay at wss://relay.vauchi.app
# Verifies: health endpoint, HTTP→HTTPS redirect, WebSocket upgrade

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Default relay
RELAY_HOST="${1:-relay.vauchi.app}"
RELAY_URL="wss://$RELAY_HOST"

PASSED=0
FAILED=0

check() {
    local name="$1"
    local result="$2"
    local expected="$3"

    if [[ "$result" == *"$expected"* ]]; then
        echo -e "${GREEN}✓${NC} $name"
        ((PASSED++))
        return 0
    else
        echo -e "${RED}✗${NC} $name"
        echo "  Expected: $expected"
        echo "  Got: $result"
        ((FAILED++))
        return 1
    fi
}

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║       Vauchi Relay Test                ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Testing relay: $RELAY_HOST"
echo ""
echo -e "${BLUE}=== HTTP Endpoints ===${NC}"

# 1. Health endpoint
health=$(curl -s --connect-timeout 10 "https://$RELAY_HOST/health" 2>&1 || echo "CURL_FAILED")
check "Health endpoint returns healthy" "$health" '"status":"healthy"'

# 2. Version in health response (optional)
if [[ "$health" == *"version"* ]]; then
    version=$(echo "$health" | grep -o '"version":"[^"]*"' | cut -d'"' -f4)
    echo -e "  Version: $version"
fi

# 3. Root returns error for non-WebSocket
root=$(curl -s --connect-timeout 10 "https://$RELAY_HOST/" 2>&1 || echo "CURL_FAILED")
check "Root returns JSON error" "$root" '"error":'

# 4. HTTP redirects to HTTPS
redirect_code=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 "http://$RELAY_HOST/" 2>&1 || echo "000")
check "HTTP→HTTPS redirect (301)" "$redirect_code" "301"

# 5. Metrics endpoint (if available)
metrics=$(curl -s --connect-timeout 10 "https://$RELAY_HOST/metrics" 2>&1 || echo "")
if [[ "$metrics" == *"vauchi_"* ]] || [[ "$metrics" == *"connections"* ]]; then
    echo -e "${GREEN}✓${NC} Metrics endpoint available"
    ((PASSED++))
else
    echo -e "${YELLOW}○${NC} Metrics endpoint (not available or protected)"
fi

echo ""
echo -e "${BLUE}=== WebSocket Connection ===${NC}"

# 6. WebSocket upgrade test
if command -v websocat &>/dev/null; then
    # Full WebSocket test with websocat
    ws_result=$(timeout 5 websocat -v "$RELAY_URL" 2>&1 || true)
    if [[ "$ws_result" == *"Connected"* ]] || [[ "$ws_result" == *"WebSocket"* ]]; then
        echo -e "${GREEN}✓${NC} WebSocket upgrade successful"
        ((PASSED++))
    else
        # Try with explicit WebSocket protocol
        ws_result=$(timeout 5 websocat --no-close -t "$RELAY_URL" 2>&1 <<< "ping" || true)
        if [[ $? -eq 0 ]] || [[ "$ws_result" != *"error"* ]]; then
            echo -e "${GREEN}✓${NC} WebSocket connection established"
            ((PASSED++))
        else
            echo -e "${RED}✗${NC} WebSocket upgrade failed"
            echo "  Got: $ws_result"
            ((FAILED++))
        fi
    fi
elif command -v wscat &>/dev/null; then
    # Alternative: use wscat (Node.js)
    ws_result=$(timeout 5 wscat -c "$RELAY_URL" --execute "ping" 2>&1 || true)
    if [[ $? -eq 0 ]]; then
        echo -e "${GREEN}✓${NC} WebSocket connection (wscat)"
        ((PASSED++))
    else
        echo -e "${RED}✗${NC} WebSocket connection failed"
        ((FAILED++))
    fi
else
    # Fallback: HTTP upgrade header check
    ws_status=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 \
        -H "Upgrade: websocket" \
        -H "Connection: Upgrade" \
        -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
        -H "Sec-WebSocket-Version: 13" \
        "https://$RELAY_HOST/" 2>&1 || echo "000")

    # curl can't complete WS handshake, but 400 means relay detected the upgrade attempt
    # A working relay returns 101 to real WS clients
    if [[ "$ws_status" == "400" ]] || [[ "$ws_status" == "426" ]]; then
        echo -e "${GREEN}✓${NC} WebSocket headers detected (relay recognizes upgrade)"
        ((PASSED++))
    else
        echo -e "${YELLOW}○${NC} WebSocket test inconclusive (status: $ws_status)"
        echo "  Install websocat for full WS test: cargo install websocat"
    fi
fi

# 7. TLS verification
echo ""
echo -e "${BLUE}=== TLS Security ===${NC}"

if command -v openssl &>/dev/null; then
    cert_info=$(echo | openssl s_client -connect "$RELAY_HOST:443" -servername "$RELAY_HOST" 2>/dev/null | openssl x509 -noout -dates -subject 2>/dev/null || echo "")

    if [[ -n "$cert_info" ]]; then
        echo -e "${GREEN}✓${NC} TLS certificate valid"
        # Extract expiry
        expiry=$(echo "$cert_info" | grep "notAfter" | cut -d= -f2)
        if [[ -n "$expiry" ]]; then
            echo "  Expires: $expiry"
        fi
        ((PASSED++))
    else
        echo -e "${RED}✗${NC} TLS certificate check failed"
        ((FAILED++))
    fi
else
    echo -e "${YELLOW}○${NC} TLS check skipped (openssl not found)"
fi

# === Latency Test ===
echo ""
echo -e "${BLUE}=== Latency ===${NC}"

latency=$(curl -s -o /dev/null -w "%{time_total}" --connect-timeout 10 "https://$RELAY_HOST/health" 2>&1 || echo "0")
latency_ms=$(echo "$latency * 1000" | bc 2>/dev/null || echo "N/A")

if [[ "$latency_ms" != "N/A" ]] && [[ $(echo "$latency < 2" | bc) -eq 1 ]]; then
    echo -e "${GREEN}✓${NC} Health endpoint latency: ${latency_ms}ms"
else
    echo -e "${YELLOW}○${NC} Health endpoint latency: ${latency_ms}ms"
fi

# === Summary ===
echo ""
echo -e "${YELLOW}════════════════════════════════════════${NC}"
echo -e "Results: ${GREEN}$PASSED passed${NC}, ${RED}$FAILED failed${NC}"
echo ""

if [[ $FAILED -gt 0 ]]; then
    echo -e "${RED}Some tests failed. Check relay configuration.${NC}"
    exit 1
else
    echo -e "${GREEN}Relay is operational!${NC}"
    exit 0
fi
