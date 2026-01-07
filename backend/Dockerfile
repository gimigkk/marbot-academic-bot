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

# Add this line - SQLx needs it during build
ARG DATABASE_URL
ENV DATABASE_URL=$DATABASE_URL

# Set SQLx to offline mode false
ENV SQLX_OFFLINE=false

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
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/marbot /app/marbot
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/schedule.json /app/schedule.json

EXPOSE 3000

CMD ["/app/marbot"]