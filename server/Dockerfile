FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev sqlite-dev openssl-dev openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache deps layer
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY build.rs ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
COPY static ./static
COPY assets ./assets
RUN touch src/main.rs && cargo build --release

FROM alpine:3.20
RUN apk add --no-cache sqlite-libs ca-certificates ffmpeg python3
# Alpine's yt-dlp package lags far behind YouTube changes (3.20 ships
# 2024.12.03, which returns no entries for playlist pages). Install the
# official release instead. Pass --build-arg YT_DLP_VERSION=<tag> to pin, or
# rebuild with --no-cache to pick up a newer "latest".
ARG YT_DLP_VERSION=latest
RUN if [ "$YT_DLP_VERSION" = "latest" ]; then \
      url=https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp; \
    else \
      url=https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/yt-dlp; \
    fi && \
    wget -q -O /usr/local/bin/yt-dlp "$url" && \
    chmod +x /usr/local/bin/yt-dlp && \
    yt-dlp --version
WORKDIR /app
COPY --from=builder /app/target/release/tubemin ./tubemin
COPY --from=builder /app/migrations ./migrations
COPY --from=builder /app/templates ./templates
COPY static ./static
EXPOSE 3000
CMD ["./tubemin"]
