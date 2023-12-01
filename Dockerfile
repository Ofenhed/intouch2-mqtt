ARG BUILD_FROM
FROM rust:alpine as base
RUN rustup toolchain install nightly && rustup default nightly
RUN apk add --no-cache musl-dev

RUN mkdir /build/
WORKDIR /build/
COPY Cargo.lock Cargo.toml ./
RUN cargo new --lib intouch2-mqtt
COPY intouch2 ./intouch2
RUN cargo build

COPY intouch2-mqtt ./intouch2-mqtt
RUN cargo build --bin intouch2-mqtt

FROM ${BUILD_FROM}
COPY --from=base /build/target/debug/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
