ARG BUILD_FROM
FROM rustlang/rust:nightly as base

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
COPY --from=base /build/target/release/intouch2-mqtt /intouch2-mqtt
CMD [ "/intouch2-mqtt" ]
