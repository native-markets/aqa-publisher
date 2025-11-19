# Multi-stage build for AQA Publisher Service
# Stage 1: Build the application
FROM rust:1.91-slim AS builder

# Install required system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the aqa-publisher project files
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build the release binary
RUN cargo build --release --bin publish_daemon

# Stage 2: Create minimal runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user for security
RUN useradd -m -u 1000 -s /bin/bash publisher

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /build/target/release/publish_daemon /app/publish_daemon

# Change ownership to non-root user
RUN chown -R publisher:publisher /app

# Switch to non-root user
USER publisher

# Run the service
CMD ["/app/publish_daemon"]
