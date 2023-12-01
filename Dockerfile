FROM rustlang/rust:nightly as base

RUN apt update -qq && apt install -y -qq --no-install-recommends \
	musl-tools

ARG BUILD_ARCH
RUN rustup set profile minimal && rustup target add ${BUILD_ARCH}-unknown-linux-musl && rustup default ${BUILD_ARCH}-nightly-unknown-linux-musl

RUN mkdir /build/
ADD intouch2 /build/intouch2
ADD Cargo.lock /build/intouch2/
WORKDIR /build/intouch2/
RUN cargo build --release && mv /build/intouch2/target /build/intouch2/Cargo.lock /build/

WORKDIR /build/
ADD Cargo.toml /build/
ADD intouch2-mqtt /build/intouch2-mqtt
RUN cargo build --bin intouch2-mqtt --release

ARG BUILD_FROM
FROM ${BUILD_FROM}
# RUN apk add libgcc
COPY --from=base /build/target/release/intouch2-mqtt /bin/intouch2-mqtt
CMD [ "/bin/intouch2-mqtt" ]
