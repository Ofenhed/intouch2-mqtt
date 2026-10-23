ARG BUILD_ARCH
ARG BUILD_FROM=ghcr.io/home-assistant/base-ubuntu:latest
FROM rust:latest AS builder
RUN rustup toolchain install nightly && rustup default nightly
RUN mkdir /build/
WORKDIR /build/

COPY ./ /build/

RUN cargo build --release --bin intouch2-mqtt

FROM ${BUILD_FROM}
ARG BUILD_VERSION
ARG BUILD_ARCH
COPY --from=builder --chmod=555 /build/target/debug/intouch2-mqtt /usr/local/bin/intouch2-mqtter
COPY docker-entrypoint.sh /docker-entrypoint.sh
EXPOSE 10022/udp
ENV RUST_BACKTRACE=1
LABEL \
  io.hass.version="${VERSION}" \
  io.hass.type="addon" \
  io.hass.arch="${BUILD_ARCH}"

CMD [ "/docker-entrypoint.sh" ]
