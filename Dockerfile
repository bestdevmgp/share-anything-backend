FROM rust:latest AS builder

RUN rustup target add x86_64-unknown-linux-musl
RUN apt-get update && apt-get install -y musl-tools build-essential gcc-x86-64-linux-gnu

WORKDIR /app
COPY . .

RUN cargo build --target x86_64-unknown-linux-musl --release

FROM debian:buster-slim

WORKDIR /app

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/프로젝트명 ./app

EXPOSE 8080

CMD ["./app"]
