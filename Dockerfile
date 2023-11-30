FROM rustlang/rust:nightly as base
ADD . .
RUN cargo build -p intouch2 --release
RUN cargo build --bin intouch2-mqtt --release

ARG BUILD_FROM
FROM $BUILD_FROM
COPY --from=base target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
