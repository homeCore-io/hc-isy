# =============================================================================
# hc-isy — HomeCore ISY/IoX Plugin
# Alpine Linux — minimal, static-friendly runtime
# =============================================================================
#
# Build:
#   docker build -t hc-isy:latest .
#
# Run:
#   docker run -d \
#     -v ./config/config.toml:/opt/hc-isy/config/config.toml:ro \
#     -v hc-isy-logs:/opt/hc-isy/logs \
#     hc-isy:latest
#
# Volumes:
#   /opt/hc-isy/config   config.toml (ISY IP, credentials)
#   /opt/hc-isy/logs     rolling log files
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1 — Build
# -----------------------------------------------------------------------------
FROM rust:1.95-alpine3.23@sha256:606fd313a0f49743ee2a7bd49a0914bab7deedb12791f3a846a34a4711db7ed2 AS builder

RUN apk upgrade --no-cache && apk add --no-cache musl-dev openssl-dev pkgconfig

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

RUN cargo build --release --bin hc-isy

# -----------------------------------------------------------------------------
# Stage 2 — Runtime
# -----------------------------------------------------------------------------
FROM alpine:3.23@sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11

# `apk upgrade` first pulls CVE patches for packages baked into the
# alpine:3 base since the upstream image was last rebuilt. Defense
# in depth — without this, `apk add --no-cache` only refreshes the
# named packages, leaving busybox/musl/etc. on the base's frozen
# versions.
RUN apk upgrade --no-cache && \
    apk add --no-cache \
        ca-certificates \
        libssl3 \
        tzdata

RUN adduser -D -h /opt/hc-isy hcisy

COPY --from=builder /build/target/release/hc-isy /usr/local/bin/hc-isy
RUN chmod 755 /usr/local/bin/hc-isy

RUN mkdir -p /opt/hc-isy/config /opt/hc-isy/logs

COPY config/config.toml.example /opt/hc-isy/config/config.toml.example

RUN chown -R hcisy:hcisy /opt/hc-isy

USER hcisy
WORKDIR /opt/hc-isy

VOLUME ["/opt/hc-isy/config", "/opt/hc-isy/logs"]

ENV RUST_LOG=info

ENTRYPOINT ["hc-isy"]
