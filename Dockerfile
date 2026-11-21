# Stage 1: Build optimized Rust binary
FROM rust:1.85-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY static ./static

RUN cargo build --release

# Stage 2: Minimal secure runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/oyun-bulucu /usr/local/bin/oyun-bulucu

ENV PORT=10000
EXPOSE 10000

CMD ["oyun-bulucu"]
