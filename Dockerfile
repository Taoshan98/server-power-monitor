# Universal Light Dockerfile (NVIDIA & Intel compatible)
FROM debian:bookworm-slim

# Set locale to C for consistent numeric parsing
ENV LC_NUMERIC=C

# Install dependencies
RUN apt-get update && apt-get install -y \
    bash \
    curl \
    gawk \
    hdparm \
    sed \
    grep \
    coreutils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the script
COPY server-power-monitor.sh .
RUN chmod +x server-power-monitor.sh

# The script expects these directories
RUN mkdir -p /app/state

# Environment variables for configuration paths
ENV STATE_DIR=/app/state
ENV LOG_FILE=/app/server-power-monitor.log
ENV CONFIG_FILE=/etc/server-power-monitor.conf

# Run the script with line-buffering to ensure logs appear immediately
ENTRYPOINT ["stdbuf", "-oL", "-eL", "/bin/bash", "server-power-monitor.sh"]

