# Build stage
FROM rust:1.92-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY schedule.json ./schedule.json
COPY ramadhan_dramaga.json ./ramadhan_dramaga.json  

RUN apt-get update && apt-get install -y screen && rm -rf /var/lib/apt/lists/*

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Build in release mode (connects to DB to verify queries)
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    screen \
    && rm -rf /var/lib/apt/lists/*

# Copy binary and files from builder
COPY --from=builder /app/target/release/marbot /app/marbot
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/schedule.json /app/schedule.json
COPY --from=builder /app/ramadhan_dramaga.json /app/ramadhan_dramaga.json 

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
  CMD curl -f http://localhost:3000/health || exit 1

CMD ["screen", "-DmS", "marbot", "/app/marbot"]