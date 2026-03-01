FROM --platform=linux/amd64 rust:latest AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY . .
RUN touch src/main.rs
RUN cargo build --release

FROM --platform=linux/amd64 debian:sid-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates ffmpeg poppler-utils && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/share-anything ./share-anything

EXPOSE 8080

CMD ["./share-anything"]