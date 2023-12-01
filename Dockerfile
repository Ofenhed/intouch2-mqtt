ARG BUILD_FROM
FROM rust:alpine as base
RUN rustup toolchain install nightly && rustup default nightly
RUN apk add --no-cache musl-dev

RUN mkdir /build/
WORKDIR /build/
ADD Cargo.lock intouch2 /build/
COPY <<EOF Cargo.toml
[workspace]
members = [
  "intouch2",
]
EOF
RUN cargo build --release

ADD Cargo.toml intouch-mqtt /build/
RUN cargo build --bin intouch2-mqtt --release

FROM ${BUILD_FROM}
COPY --from=base /build/target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
