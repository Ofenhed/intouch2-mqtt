ARG BUILD_ARCH
ARG BUILD_FROM=ghcr.io/home-assistant/base-ubuntu:latest
FROM rust:latest AS builder
RUN rustup toolchain install nightly && rustup default nightly

COPY . /build/
WORKDIR /build/

RUN --mount=type=cache,target=./target,id=rust-target,sharing=locked cargo build --release --bin intouch2-mqtt && cp target/release/intouch2-mqtt /

FROM ${BUILD_FROM}
ARG BUILD_VERSION
ARG BUILD_ARCH
EXPOSE 10022/udp
LABEL \
  io.hass.version="${BUILD_VERSION}" \
  io.hass.type="addon" \
  io.hass.arch="${BUILD_ARCH}"
COPY --from=builder --chmod=555 /intouch2-mqtt /usr/local/bin/intouch2-mqtt
COPY docker-entrypoint.sh /docker-entrypoint.sh

CMD [ "/docker-entrypoint.sh" ]
