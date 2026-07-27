# Multi-stage Dockerfile for Server Power Monitor (Rust Edition)

# --- Stage 1: Build binary ---
FROM rust:latest AS builder

WORKDIR /usr/src/app

# Install build tools and SSL libraries
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests
COPY Cargo.toml Cargo.lock* ./

# Copy source code and build release binary
COPY src ./src
RUN cargo build --release

# --- Stage 2: Minimal runtime image ---
FROM debian:bookworm-slim

ENV LC_NUMERIC=C

# Install runtime dependencies (SSL certificates, hdparm for disk power detection)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    hdparm \
    procps \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from build stage
COPY --from=builder /usr/src/app/target/release/server-power-monitor /app/server-power-monitor

# Create state directory
RUN mkdir -p /app/state

ENV STATE_DIR=/app/state
ENV LOG_FILE=/app/server-power-monitor.log
ENV CONFIG_FILE=/etc/server-power-monitor.conf

ENTRYPOINT ["/app/server-power-monitor"]
