ARG BUILD_FROM
FROM rust:alpine as base
RUN rustup toolchain install nightly && rustup default nightly

RUN mkdir /build/
ADD intouch2 /build/intouch2
ADD Cargo.lock /build/intouch2/
WORKDIR /build/intouch2/
RUN cargo build --release && mv /build/intouch2/target /build/intouch2/Cargo.lock /build/

WORKDIR /build/
ADD Cargo.toml /build/
ADD intouch2-mqtt /build/intouch2-mqtt
RUN cargo build --bin intouch2-mqtt --release

FROM ${BUILD_FROM}
# RUN apk add libgcc
COPY --from=base /build/target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
