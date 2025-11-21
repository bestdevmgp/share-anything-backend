FROM rust:latest AS builder

RUN apt-get update && apt-get install -y musl-tools pkg-config libssl-dev build-essential clang
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY . .

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV RUSTFLAGS='-C target-feature=+crt-static'

RUN cargo build --target x86_64-unknown-linux-musl --release

FROM debian:buster-slim
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/share-anything ./share-anything

EXPOSE 8080

CMD ["./share-anything"]
