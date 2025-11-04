ARG BUILD_ARCH=amd64
ARG BUILD_FROM=ghcr.io/home-assistant/${BUILD_ARCH}-base-ubuntu:latest
FROM rust:latest AS build
RUN rustup toolchain install nightly && rustup default nightly
RUN mkdir /build/
WORKDIR /build/

COPY Cargo.lock Cargo.toml ./

RUN cargo new --bin intouch2-mqtt
RUN cargo new --lib intouch2
COPY intouch2/Cargo.toml ./intouch2/Cargo.toml
COPY intouch2-mqtt/Cargo.toml ./intouch2-mqtt/Cargo.toml
RUN cargo build --release

COPY intouch2/ ./intouch2/
RUN touch ./intouch2/src/* && cargo build --release -p intouch2

COPY intouch2-mqtt/ ./intouch2-mqtt/
RUN touch ./intouch2-mqtt/src/* && cargo build --release --bin intouch2-mqtt

FROM ${BUILD_FROM}
COPY --from=build --chmod=555 /build/target/release/intouch2-mqtt /usr/local/bin/intouch2-mqtt
COPY docker-entrypoint.sh /docker-entrypoint.sh
EXPOSE 10022/udp

CMD [ "/docker-entrypoint.sh" ]
