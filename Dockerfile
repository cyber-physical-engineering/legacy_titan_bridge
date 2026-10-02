# ==============================================================================
# Legacy Titan Bridge - Multi-Stage Docker Build
# ==============================================================================
# Stage 1: Compile COBOL library using GnuCOBOL
# Stage 2: Build Rust sidecar with release optimizations
# Stage 3: Minimal runtime image with both artifacts
# ==============================================================================

# ------------------------------------------------------------------------------
# Stage 1: COBOL Compilation
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim AS cobol-builder

# Install GnuCOBOL
RUN apt-get update && apt-get install -y --no-install-recommends \
    gnucobol \
    gcc \
    libc6-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy COBOL source
COPY cobol/ ./cobol/

# Compile COBOL to shared library
RUN cd cobol && \
    cobc -m -o libcorebanking.so core_banking.cbl && \
    ls -la libcorebanking.so

# ------------------------------------------------------------------------------
# Stage 2: Rust Compilation
# ------------------------------------------------------------------------------
FROM rust:1-slim-bookworm AS rust-builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies by building them first
COPY sidecar/Cargo.toml sidecar/Cargo.lock* ./sidecar/
RUN mkdir -p sidecar/src && \
    echo 'fn main() { println!("Placeholder"); }' > sidecar/src/main.rs && \
    cd sidecar && cargo build --release && \
    rm -rf src

# Copy actual source and build
COPY sidecar/ ./sidecar/
RUN cd sidecar && \
    touch src/main.rs && \
    cargo build --release && \
    strip target/release/titan-sidecar

# ------------------------------------------------------------------------------
# Stage 3: Runtime Image
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    libcob4 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -s /bin/false titan

# Create directories
RUN mkdir -p /app /usr/lib/titan

WORKDIR /app

# Copy COBOL library from Stage 1
COPY --from=cobol-builder /build/cobol/libcorebanking.so /usr/lib/titan/

# Copy Rust binary from Stage 2
COPY --from=rust-builder /build/sidecar/target/release/titan-sidecar /app/

# Set library path
ENV LD_LIBRARY_PATH=/usr/lib/titan
ENV COBOL_LIB_PATH=/usr/lib/titan/libcorebanking.so

# Configure runtime
ENV SIDECAR_PORT=3000
ENV RUST_LOG=info

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Run as non-root user
USER titan

# Start the sidecar
CMD ["/app/titan-sidecar"]
