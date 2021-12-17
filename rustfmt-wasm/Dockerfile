FROM rust:1.57.0-alpine3.14

RUN apk add --no-cache libc-dev npm
RUN wget https://rustwasm.github.io/wasm-pack/installer/init.sh -O - | sh

WORKDIR /root
COPY . .
RUN CFG_RELEASE=nightly wasm-pack build --release

ENTRYPOINT ["/bin/sh"]
