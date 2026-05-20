FROM i386/debian:bookworm-slim

ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:${PATH}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    libatomic1 \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
       sh -s -- -y --default-host i686-unknown-linux-gnu --default-toolchain 1.95.0 --profile minimal \
    && rustup target add wasm32-unknown-unknown \
    && apt-get purge -y --auto-remove curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
       /usr/local/rustup/toolchains/*/share/doc \
       /usr/local/rustup/toolchains/*/share/man \
       /usr/local/rustup/toolchains/*/etc \
       /usr/local/cargo/registry \
       /usr/local/cargo/.package-cache \
       /tmp/* \
       /root/.cargo

COPY <<'SCRIPT' /usr/local/bin/compile-wasm
#!/bin/sh
rustc --target wasm32-unknown-unknown --edition 2024 -C opt-level=0 -C debuginfo=0 -C codegen-units=256 -o /tmp/out.wasm "$@" 2>&1
SCRIPT
RUN chmod +x /usr/local/bin/compile-wasm

RUN rustc --version && \
    rustc --print target-list | grep wasm32-unknown-unknown

ENTRYPOINT ["/bin/sh"]
