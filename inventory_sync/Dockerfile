# Build stage
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests and source
COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Build the application
RUN cargo build --release

# Test stage - used by CI to run tests in the same environment as production
FROM rust:1.83-slim-bookworm AS tester

WORKDIR /app

# Install build/test dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install additional tools for CI
RUN rustup component add rustfmt clippy && \
    cargo install cargo-audit

# Copy manifests and source
COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Default command runs full test suite
CMD ["sh", "-c", "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --verbose && cargo build --release && cargo audit"]

# Runtime stage
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1000 appuser

# Create data directory for SQLite
RUN mkdir -p /data && chown appuser:appuser /data

# Copy binary from builder
COPY --from=builder /app/target/release/inventory_sync /usr/local/bin/

# Switch to non-root user
USER appuser

# Default database path inside container
ENV RUST_LOG=info

# Mount point for persistent data
VOLUME ["/data"]

# Run with database in mounted volume
ENTRYPOINT ["inventory_sync", "--database", "/data/inventory.db"]
