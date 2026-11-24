# syntax=docker/dockerfile:1

FROM rust:1.98-bookworm AS builder
WORKDIR /build

# The sqlx query! macros are checked at compile time. .sqlx holds the prepared
# metadata, so the image builds without a live database.
ENV SQLX_OFFLINE=true

COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY migrations ./migrations
COPY templates ./templates
COPY public ./public
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/miniblog /usr/local/bin/miniblog
COPY --from=builder /build/public ./public

USER 10001:10001
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=15s --retries=3 \
    CMD ["curl", "-fsS", "http://127.0.0.1:3000/health"]

ENTRYPOINT ["/usr/local/bin/miniblog"]
