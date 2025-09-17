# 1. Use Rust official image to build the app
FROM rust:1.82 as builder

WORKDIR /app
COPY . .

# Create release build
RUN cargo build --release

# 2. Minimal runtime image
FROM debian:bullseye-slim

# Install OpenSSL and ca-certificates (needed by sqlx + rpc client)
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/indexer /app/indexer

# Expose the app port
EXPOSE 3000

# Run the app
CMD ["/app/indexer"]
