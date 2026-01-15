# Build stage
FROM rust:1.75-bookworm AS builder

WORKDIR /app

# Copy the entire workspace (needed for workspace dependencies)
COPY Cargo.toml ./
COPY webbook-core ./webbook-core
COPY webbook-relay ./webbook-relay

# Build release binary
RUN cargo build --release -p webbook-relay

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies (OpenSSL for TLS)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/webbook-relay /usr/local/bin/

# Create non-root user
RUN useradd -r -s /bin/false webbook
USER webbook

# Expose default port
EXPOSE 8080

# Environment variables with defaults
ENV RELAY_LISTEN_ADDR=0.0.0.0:8080
ENV RELAY_MAX_CONNECTIONS=1000
ENV RELAY_MAX_MESSAGE_SIZE=1048576
ENV RELAY_BLOB_TTL_SECS=604800
ENV RELAY_RATE_LIMIT=60
ENV RELAY_CLEANUP_INTERVAL=3600
ENV RUST_LOG=webbook_relay=info

CMD ["webbook-relay"]
