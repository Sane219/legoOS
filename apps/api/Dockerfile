FROM rust:1.85-slim AS build
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY apps/api/Cargo.toml apps/api/Cargo.toml
COPY apps/api/src apps/api/src
COPY apps/api/migrations apps/api/migrations
COPY apps/executor apps/executor
COPY apps/llm apps/llm
COPY apps/mcp apps/mcp
COPY apps/queue apps/queue
COPY apps/rag apps/rag
COPY apps/worker/Cargo.toml apps/worker/Cargo.toml
COPY apps/worker/src apps/worker/src

RUN cargo build --release --manifest-path apps/api/Cargo.toml

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# sqlx::migrate! embeds migrations/ into the binary at compile time, so it isn't needed here.
COPY --from=build /build/target/release/api /app/api

EXPOSE 8080
CMD ["/app/api"]
