FROM rustlang/rust:nightly as base
RUN cargo install cargo-build-dependencies
ADD Cargo.toml Cargo.lock ./
RUN mkdir intouch2 intouch2-mqtt
ADD intouch2/Cargo.toml intouch2/Cargo.toml
ADD intouch2-mqtt/Cargo.toml intouch2-mqtt/Cargo.toml
RUN cargo build-dependencies --release
ADD intouch2 intouch2-mqtt ./
RUN cargo build --bin intouch2-mqtt --release

ARG BUILD_FROM
FROM $BUILD_FROM
COPY --from=base target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
