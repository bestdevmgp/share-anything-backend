FROM --platform=linux/amd64 rust:latest AS builder

WORKDIR /app

COPY . .

RUN cargo build --release

FROM --platform=linux/amd64 debian:sid-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/share-anything ./share-anything

EXPOSE 8080

CMD ["./share-anything"]