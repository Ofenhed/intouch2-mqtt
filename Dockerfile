FROM rustlang/rust:nightly as base

RUN mkdir /build/
WORKDIR /build/
ADD . .
RUN cargo build --bin intouch2-mqtt --release

FROM alpine:latest
COPY --from=base /build/target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
