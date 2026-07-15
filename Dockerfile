FROM rust:1.96.0-alpine3.23@sha256:5dc2af9dd547c33f64d5fc1d299ab93b51f39eaa16c426c476b990ce6caf5b3e AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --locked --release

FROM scratch AS runtime
ARG VCS_REF=unknown
LABEL org.opencontainers.image.source="https://github.com/hbjoroy/sproyt" \
      org.opencontainers.image.revision="$VCS_REF"
WORKDIR /app
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /src/target/release/sproyt /app/sproyt
USER 65532:65532
EXPOSE 9010
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    SPROYT_ADDR=0.0.0.0:9010 \
    SPROYT_ENV=production \
    SPROYT_LOG_FORMAT=json
ENTRYPOINT ["/app/sproyt"]
