FROM rust:1.96.0-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 65532 sproyt \
    && useradd --system --uid 65532 --gid sproyt --home-dir /nonexistent sproyt
WORKDIR /app
COPY --from=builder /src/target/release/sproyt /app/sproyt
USER 65532:65532
EXPOSE 9010
ENV SPROYT_ADDR=0.0.0.0:9010 \
    SPROYT_ENV=production \
    SPROYT_LOG_FORMAT=json
ENTRYPOINT ["/app/sproyt"]
