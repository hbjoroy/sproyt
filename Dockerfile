FROM rust:1.96.0-bookworm@sha256:5e2214abe154fe26e39f64488952e5c991eeed1d6d6da7cc8381ae83927f0cfc AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --locked --release

FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df AS runtime
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
