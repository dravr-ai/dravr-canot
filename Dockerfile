# ABOUTME: Multi-stage Docker build for dravr-canot-server and dravr-canot-mcp binaries
# ABOUTME: Runtime uses debian:bookworm-slim with ca-certificates for TLS
#
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 dravr.ai

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p dravr-canot-server -p dravr-canot-mcp --features all-channels

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash dravr

COPY --from=builder /build/target/release/dravr-canot-server /usr/local/bin/
COPY --from=builder /build/target/release/dravr-canot-mcp /usr/local/bin/

USER dravr
WORKDIR /home/dravr

EXPOSE 3000
ENTRYPOINT ["dravr-canot-server"]
CMD ["--host", "0.0.0.0"]
