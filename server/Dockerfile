FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev sqlite-dev openssl-dev openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache deps layer
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
COPY static ./static
RUN touch src/main.rs && cargo build --release

FROM alpine:3.20
RUN apk add --no-cache sqlite-libs ca-certificates
WORKDIR /app
COPY --from=builder /app/target/release/tubemin ./tubemin
COPY --from=builder /app/migrations ./migrations
COPY --from=builder /app/templates ./templates
COPY static ./static
EXPOSE 3000
CMD ["./tubemin"]
