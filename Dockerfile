ARG BUILD_ARCH
ARG BUILD_FROM=ghcr.io/home-assistant/${BUILD_ARCH}
FROM rust:alpine as base
RUN rustup toolchain install nightly && rustup default nightly
RUN apk add --no-cache musl-dev

RUN mkdir /build/
WORKDIR /build/

COPY Cargo.lock Cargo.toml ./
RUN cargo new --bin intouch2-mqtt
RUN cargo new --lib intouch2
COPY intouch2/Cargo.toml ./intouch2/Cargo.toml
COPY intouch2-mqtt/Cargo.toml ./intouch2-mqtt/Cargo.toml
RUN cargo build

COPY intouch2/ ./intouch2/
RUN touch ./intouch2/src/* && cargo build -p intouch2

COPY intouch2-mqtt/ ./intouch2-mqtt/
RUN touch ./intouch2-mqtt/src/* && cargo build --bin intouch2-mqtt

FROM ${BUILD_FROM}
RUN apk add --no-cache tini
COPY --from=base /build/target/debug/intouch2-mqtt /usr/local/bin/intouch2-mqtt
COPY docker-entrypoint.sh /docker-entrypoint.sh
EXPOSE 10022/udp
ENTRYPOINT [ "/sbin/tini", "--", "/docker-entrypoint.sh" ]
