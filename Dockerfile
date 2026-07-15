# syntax=docker/dockerfile:1

ARG BUILDPLATFORM
FROM --platform=$BUILDPLATFORM rust:1.96.0-alpine3.23@sha256:5dc2af9dd547c33f64d5fc1d299ab93b51f39eaa16c426c476b990ce6caf5b3e AS builder
ARG TARGETARCH
WORKDIR /src
RUN apk add --no-cache zig=0.15.2-r0 \
    && cargo install --locked --version 0.23.0 cargo-zigbuild
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN case "$TARGETARCH" in \
      amd64) rust_target=x86_64-unknown-linux-musl ;; \
      arm64) rust_target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$rust_target" \
    && cargo zigbuild --locked --release --target "$rust_target" \
    && install -D "target/$rust_target/release/sproyt" /out/sproyt

FROM scratch AS runtime
ARG VCS_REF=unknown
LABEL org.opencontainers.image.source="https://github.com/hbjoroy/sproyt" \
      org.opencontainers.image.revision="$VCS_REF"
WORKDIR /app
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /out/sproyt /app/sproyt
USER 65532:65532
EXPOSE 9010
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    SPROYT_ADDR=0.0.0.0:9010 \
    SPROYT_ENV=production \
    SPROYT_LOG_FORMAT=json
ENTRYPOINT ["/app/sproyt"]
