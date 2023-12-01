FROM rustlang/rust:nightly as base

RUN mkdir /build/
ADD intouch2 /build/
ADD Cargo.lock /build/intouch2/
WORKDIR /build/intouch2/
RUN cargo build --release && mv /build/intouch2/target /build/
WORKDIR /build/intouch2/
ADD intouch2-mqtt Cargo.toml Cargo.lock /build/
RUN cargo build --bin intouch2-mqtt --release

FROM alpine:latest
COPY --from=base /build/target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
