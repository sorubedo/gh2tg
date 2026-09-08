# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

WORKDIR /build

RUN apt-get update \
    && apt-get install --no-install-recommends -y pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/gh2tg /usr/local/bin/gh2tg

WORKDIR /data

FROM runtime AS cron

RUN apt-get update \
    && apt-get install --no-install-recommends -y cron \
    && rm -rf /var/lib/apt/lists/*

COPY docker/cron-entrypoint.sh /usr/local/bin/gh2tg-cron
RUN chmod 0755 /usr/local/bin/gh2tg-cron

ENTRYPOINT ["/usr/local/bin/gh2tg-cron"]

FROM runtime AS one-shot

ENTRYPOINT ["/usr/local/bin/gh2tg"]
