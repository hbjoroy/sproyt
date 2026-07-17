FROM docker.io/library/rust:1.96-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --locked -p heart-api

FROM docker.io/library/debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/heart-api /usr/local/bin/heart-api
EXPOSE 8080
ENTRYPOINT ["heart-api"]
