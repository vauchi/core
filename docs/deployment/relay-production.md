# WebBook Relay Production Deployment Guide

This guide covers deploying the WebBook Relay server in a production environment with TLS encryption.

## Prerequisites

- A server with a public IP address
- A domain name pointed to your server (e.g., `relay.webbook.app`)
- Docker installed (recommended) or Rust toolchain
- A reverse proxy (nginx or Caddy) for TLS termination

## Architecture Overview

```
┌──────────────────┐     HTTPS/WSS     ┌─────────────────┐     HTTP/WS      ┌───────────────┐
│  Mobile/Desktop  │ ──────────────────▶│  Reverse Proxy  │ ───────────────▶│ WebBook Relay │
│      Clients     │       :443         │  (nginx/Caddy)  │      :8080      │    Server     │
└──────────────────┘                    └─────────────────┘                  └───────────────┘
```

The relay server does not handle TLS directly. A reverse proxy terminates TLS and forwards traffic to the relay.

## Deployment Options

### Option 1: Docker + nginx (Recommended)

1. **Deploy the relay container:**

```bash
# Pull or build the image
docker build -t webbook-relay ./webbook-relay

# Create data volume
docker volume create relay-data

# Run the container
docker run -d \
  --name webbook-relay \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -p 127.0.0.1:8081:8081 \
  -v relay-data:/data \
  -e RELAY_STORAGE_BACKEND=sqlite \
  -e RELAY_MAX_CONNECTIONS=1000 \
  -e RUST_LOG=webbook_relay=info \
  webbook-relay
```

2. **Install and configure nginx:**

```bash
# Install nginx
sudo apt install nginx

# Copy the configuration
sudo cp webbook-relay/deploy/nginx/webbook-relay.conf /etc/nginx/sites-available/
sudo ln -s /etc/nginx/sites-available/webbook-relay.conf /etc/nginx/sites-enabled/

# Edit the config to set your domain
sudo nano /etc/nginx/sites-available/webbook-relay.conf
# Change: server_name relay.webbook.example.com
# To: server_name your-domain.com

# Test the configuration
sudo nginx -t

# Get TLS certificate with certbot
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d your-domain.com

# Reload nginx
sudo systemctl reload nginx
```

3. **Verify deployment:**

```bash
# Check health endpoint
curl https://your-domain.com/health

# Test WebSocket connection (requires websocat)
websocat wss://your-domain.com
```

### Option 2: Docker + Caddy

Caddy handles TLS automatically with Let's Encrypt.

1. **Deploy the relay:**

```bash
docker run -d \
  --name webbook-relay \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v relay-data:/data \
  webbook-relay
```

2. **Install and configure Caddy:**

```bash
# Install Caddy
sudo apt install caddy

# Copy and edit Caddyfile
sudo cp webbook-relay/deploy/caddy/Caddyfile /etc/caddy/
sudo nano /etc/caddy/Caddyfile
# Change relay.webbook.example.com to your domain

# Restart Caddy (it will auto-obtain certificates)
sudo systemctl restart caddy
```

### Option 3: Kubernetes with Helm

See `webbook-relay/deploy/helm/webbook-relay/` for the Helm chart.

```bash
helm install webbook-relay ./webbook-relay/deploy/helm/webbook-relay \
  --set ingress.enabled=true \
  --set ingress.hosts[0].host=relay.your-domain.com \
  --set ingress.tls[0].secretName=webbook-relay-tls \
  --set ingress.tls[0].hosts[0]=relay.your-domain.com
```

## Environment Variables

Copy the `.env.example` file and customize:

```bash
cp docs/deployment/.env.example .env
```

| Variable | Default | Description |
|----------|---------|-------------|
| `RELAY_LISTEN_ADDR` | `0.0.0.0:8080` | WebSocket listen address |
| `RELAY_MAX_CONNECTIONS` | `1000` | Maximum concurrent WebSocket connections |
| `RELAY_MAX_MESSAGE_SIZE` | `1048576` | Maximum message size (1MB) |
| `RELAY_BLOB_TTL_SECS` | `7776000` | Blob expiration (90 days) |
| `RELAY_RATE_LIMIT` | `60` | Messages per minute per client |
| `RELAY_CLEANUP_INTERVAL` | `3600` | Cleanup interval (1 hour) |
| `RELAY_STORAGE_BACKEND` | `sqlite` | `memory` or `sqlite` |
| `RELAY_DATA_DIR` | `/data` | Data directory for SQLite |
| `RUST_LOG` | `webbook_relay=info` | Log level |

## Security Checklist

- [ ] TLS 1.2+ enforced (handled by reverse proxy config)
- [ ] HTTP redirects to HTTPS
- [ ] Security headers configured (HSTS, X-Frame-Options, etc.)
- [ ] Rate limiting enabled
- [ ] Metrics endpoint restricted to internal IPs
- [ ] Firewall configured (only 80/443 open to public)
- [ ] Relay running as non-root user
- [ ] Data volume backed up regularly

## Monitoring

### Health Checks

- **Liveness:** `GET /health` - Returns 200 if server is running
- **Readiness:** `GET /ready` - Returns 200 if storage is accessible

### Prometheus Metrics

Available at `GET /metrics` on port 8081 (restricted to internal IPs):

```promql
# Active connections
relay_connections_active

# Message throughput
rate(relay_messages_received_total[5m])

# Error rate
rate(relay_connection_errors_total[5m]) / rate(relay_connections_total[5m])

# Storage utilization
relay_blobs_stored

# Rate limiting events
rate(relay_rate_limited_total[5m])
```

### Alerting Rules

Example Prometheus alerting rules:

```yaml
groups:
  - name: webbook-relay
    rules:
      - alert: RelayDown
        expr: up{job="webbook-relay"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: WebBook Relay is down

      - alert: HighErrorRate
        expr: rate(relay_connection_errors_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: High connection error rate

      - alert: HighConnectionCount
        expr: relay_connections_active > 900
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: Approaching connection limit
```

## Backup Strategy

### SQLite Database

```bash
# Stop the relay (or use SQLite backup API)
docker stop webbook-relay

# Backup the data volume
docker run --rm -v relay-data:/data -v $(pwd):/backup alpine \
  tar czf /backup/relay-backup-$(date +%Y%m%d).tar.gz /data

# Restart
docker start webbook-relay
```

### Automated Backups

Add to crontab:
```bash
0 3 * * * /path/to/backup-script.sh
```

## Troubleshooting

### Connection refused
- Check if relay container is running: `docker ps`
- Check relay logs: `docker logs webbook-relay`
- Verify port binding: `netstat -tlnp | grep 8080`

### TLS certificate errors
- Verify certificate paths in nginx/Caddy config
- Check certificate expiration: `certbot certificates`
- Renew if needed: `certbot renew`

### WebSocket connection drops
- Increase proxy timeouts in nginx/Caddy config
- Check for firewall timeout settings
- Verify `proxy_read_timeout` is sufficient (3600s recommended)

### High memory usage
- Switch from `memory` to `sqlite` storage backend
- Reduce `RELAY_BLOB_TTL_SECS` to expire data faster
- Check for connection leaks: `relay_connections_active` metric

## Additional Resources

- Detailed deployment options: `webbook-relay/deploy/DEPLOYMENT.md`
- nginx configuration: `webbook-relay/deploy/nginx/webbook-relay.conf`
- Caddy configuration: `webbook-relay/deploy/caddy/Caddyfile`
- Helm chart: `webbook-relay/deploy/helm/webbook-relay/`
- Docker Compose: `webbook-relay/docker-compose.yml`
