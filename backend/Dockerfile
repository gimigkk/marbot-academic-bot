# Your ORIGINAL Dockerfile - kept as-is for VPS fallback build
# Build stage
FROM rust:slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY schedule.json ./schedule.json
COPY .sqlx ./.sqlx 

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# ✅ Use offline mode
ENV SQLX_OFFLINE=true

# Build in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    wget \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary and files
COPY --from=builder /app/target/release/marbot /app/marbot
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/schedule.json /app/schedule.json

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
  CMD curl -f http://localhost:3000/health || exit 1

CMD ["/app/marbot"]