FROM rust:1.85-slim AS build
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY apps/worker/Cargo.toml apps/worker/Cargo.toml
COPY apps/worker/src apps/worker/src
COPY apps/executor apps/executor
COPY apps/llm apps/llm
COPY apps/queue apps/queue
COPY apps/api/Cargo.toml apps/api/Cargo.toml
COPY apps/api/src apps/api/src
COPY apps/api/migrations apps/api/migrations

RUN cargo build --release --manifest-path apps/worker/Cargo.toml

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/target/release/worker /app/worker

CMD ["/app/worker"]
