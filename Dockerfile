FROM rust:latest AS builder

RUN apt-get update && apt-get install -y musl-tools pkg-config libssl-dev
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY . .

ENV PKG_CONFIG_ALLOW_CROSS=1
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM debian:buster-slim
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/프로젝트명 ./app

EXPOSE 8080

CMD ["./app"]
