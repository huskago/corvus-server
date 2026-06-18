FROM rust:slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml ./
COPY src ./src
COPY admin ./admin
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/corvus-server /usr/local/bin/
EXPOSE 8080
VOLUME /data
WORKDIR /data
ENV CORVUS_SERVER__DATA_DIR=/data
CMD ["corvus-server"]
