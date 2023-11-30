FROM rustlang/rust:nightly as base

RUN echo '[workspace] \
members = ["intouch2"]' > Cargo.toml
ADD intouch2 Cargo.lock ./
RUN cargo build --lib --release
ADD intouch2-mqtt Cargo.toml ./
RUN cargo build --bin intouch2-mqtt --release

FROM alpine:latest
COPY --from=base target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
