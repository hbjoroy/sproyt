# syntax=docker/dockerfile:1

ARG BUILDPLATFORM
ARG BUILD_VARIANT=zig
FROM --platform=$BUILDPLATFORM rust:1.96.0-alpine3.23@sha256:5dc2af9dd547c33f64d5fc1d299ab93b51f39eaa16c426c476b990ce6caf5b3e AS build-base
WORKDIR /src
ENV SPROYT_FRONTEND_PREBUILT=1

FROM --platform=$BUILDPLATFORM node:24.19.0-alpine3.23@sha256:244cc2b53f46f9e876304391d17682b0ddae9ac33491f4857e25e35a36ba7995 AS frontend-builder
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

FROM build-base AS zig-builder
RUN --mount=type=cache,id=sproyt-cargo-registry,target=/usr/local/cargo/registry \
    apk add --no-cache zig=0.15.2-r0 \
    && cargo install --locked --version 0.23.0 cargo-zigbuild
COPY Cargo.toml Cargo.lock ./
COPY crates/sproyt-protocol/Cargo.toml crates/sproyt-protocol/Cargo.toml
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      amd64) rust_target=x86_64-unknown-linux-musl ;; \
      arm64) rust_target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$rust_target" \
    && mkdir -p src crates/sproyt-protocol/src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf 'pub fn placeholder() {}\n' > crates/sproyt-protocol/src/lib.rs \
    && cargo zigbuild --locked --release --target "$rust_target"
COPY src/. ./src/
COPY crates/sproyt-protocol/src/. ./crates/sproyt-protocol/src/
COPY build.rs ./
COPY migrations ./migrations
COPY assets ./assets
COPY --from=frontend-builder /src/frontend/dist ./frontend/dist
RUN test -f src/domain/mod.rs \
    && grep -q '^mod commands;$' crates/sproyt-protocol/src/lib.rs
ARG VCS_REF=unknown
RUN case "$TARGETARCH" in \
      amd64) rust_target=x86_64-unknown-linux-musl ;; \
      arm64) rust_target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$rust_target" \
    && cargo clean --package sproyt --package sproyt-protocol --release --target "$rust_target" \
    && SPROYT_BUILD_REVISION="$VCS_REF" cargo zigbuild --locked --release --target "$rust_target" --bin sproyt \
    && install -D "target/$rust_target/release/sproyt" /out/sproyt

FROM build-base AS native-builder
COPY Cargo.toml Cargo.lock ./
COPY crates/sproyt-protocol/Cargo.toml crates/sproyt-protocol/Cargo.toml
RUN mkdir -p src crates/sproyt-protocol/src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf 'pub fn placeholder() {}\n' > crates/sproyt-protocol/src/lib.rs \
    && cargo build --locked --release
COPY src/. ./src/
COPY crates/sproyt-protocol/src/. ./crates/sproyt-protocol/src/
COPY build.rs ./
COPY migrations ./migrations
COPY assets ./assets
COPY --from=frontend-builder /src/frontend/dist ./frontend/dist
RUN test -f src/domain/mod.rs \
    && grep -q '^mod commands;$' crates/sproyt-protocol/src/lib.rs
ARG VCS_REF=unknown
RUN cargo clean --package sproyt --package sproyt-protocol --release \
    && SPROYT_BUILD_REVISION="$VCS_REF" cargo build --locked --release --bin sproyt \
    && install -D target/release/sproyt /out/sproyt

FROM ${BUILD_VARIANT}-builder AS compiled

FROM scratch AS runtime
ARG VCS_REF=unknown
LABEL org.opencontainers.image.source="https://github.com/hbjoroy/sproyt" \
      org.opencontainers.image.revision="$VCS_REF"
WORKDIR /app
COPY --from=compiled /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=compiled /out/sproyt /app/sproyt
USER 65532:65532
EXPOSE 9010
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    SPROYT_ADDR=0.0.0.0:9010 \
    SPROYT_ENV=production \
    SPROYT_LOG_FORMAT=json
ENTRYPOINT ["/app/sproyt"]
