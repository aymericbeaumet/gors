ARG ALPINE_IMAGE=alpine:edge
ARG APK_RESOLUTION

FROM --platform=linux/386 ${ALPINE_IMAGE} AS provider-builder
ARG APK_RESOLUTION

RUN set -eu; \
    test -n "${APK_RESOLUTION}"; \
    apk update > /dev/null; \
    CURRENT_RESOLUTION=$(apk policy alpine-base cargo ca-certificates jq linux-firmware-none linux-virt mold openrc rust); \
    test "$CURRENT_RESOLUTION" = "${APK_RESOLUTION}"; \
    apk add cargo rust ca-certificates; \
    rm -rf /var/cache/apk/*
WORKDIR /build
COPY Cargo.v86.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY gors-runtime-abi gors-runtime-abi
COPY gors-runtime gors-runtime

RUN cargo build --locked --release --package gors-runtime-abi \
      --example runtime_provider_manifest \
      --example runtime_dependency_validator

RUN set -eu; \
    mkdir -p /out; \
    rustc -vV > /out/rustc-vV; \
    sed -n 's/^host: //p' /out/rustc-vV > /out/target-triple; \
    test -s /out/target-triple; \
    TARGET_TRIPLE=$(cat /out/target-triple); \
    TARGET_LIBDIR=$(rustc --target "$TARGET_TRIPLE" --print target-libdir); \
    test -d "$TARGET_LIBDIR"; \
    rustc gors-runtime/src/lib.rs \
      --crate-name __gors_runtime \
      --crate-type rlib \
      --edition 2024 \
      --target "$TARGET_TRIPLE" \
      --emit link \
      -C metadata=gors-runtime-v1 \
      -C opt-level=3 \
      -C codegen-units=1 \
      -C panic=unwind \
      -C embed-bitcode=yes \
      --remap-path-prefix=/build=/gors-workspace \
      -o /out/lib__gors_runtime.rlib; \
    cp target/release/examples/runtime_provider_manifest \
      /out/gors-runtime-provider-manifest; \
    cp target/release/examples/runtime_dependency_validator \
      /out/gors-runtime-dependency-validator; \
    /out/gors-runtime-provider-manifest \
      auto \
      /out/rustc-vV \
      "$TARGET_LIBDIR" \
      "$TARGET_TRIPLE" \
      32 \
      little \
      /out/lib__gors_runtime.rlib \
      /out/provider.json

FROM --platform=linux/386 ${ALPINE_IMAGE} AS kernel-builder
ARG APK_RESOLUTION

RUN set -eu; \
    test -n "${APK_RESOLUTION}"; \
    apk update > /dev/null; \
    CURRENT_RESOLUTION=$(apk policy alpine-base cargo ca-certificates jq linux-firmware-none linux-virt mold openrc rust); \
    test "$CURRENT_RESOLUTION" = "${APK_RESOLUTION}"; \
    apk add linux-virt linux-firmware-none; \
    rm -rf /var/cache/apk/*
RUN mkinitfs -F "base virtio 9p" $(cat /usr/share/kernel/virt/kernel.release)

FROM --platform=linux/386 ${ALPINE_IMAGE}
ARG APK_RESOLUTION

RUN set -eu; \
    test -n "${APK_RESOLUTION}"; \
    apk update > /dev/null; \
    CURRENT_RESOLUTION=$(apk policy alpine-base cargo ca-certificates jq linux-firmware-none linux-virt mold openrc rust); \
    test "$CURRENT_RESOLUTION" = "${APK_RESOLUTION}"; \
    apk add openrc alpine-base linux-virt linux-firmware-none rust mold jq; \
    rm -rf /var/cache/apk/*
COPY --from=kernel-builder /boot/initramfs-virt /boot/initramfs-virt
COPY --from=provider-builder /out/lib__gors_runtime.rlib \
    /usr/local/share/gors/runtime/lib__gors_runtime.rlib
COPY --from=provider-builder /out/provider.json \
    /usr/local/share/gors/runtime/producer.json
COPY --from=provider-builder /out/target-triple \
    /usr/local/share/gors/runtime/target-triple
COPY --from=provider-builder /out/gors-runtime-provider-manifest \
    /usr/local/libexec/gors-runtime-provider-manifest
COPY --from=provider-builder /out/gors-runtime-dependency-validator \
    /usr/local/libexec/gors-runtime-dependency-validator

COPY --chmod=755 rootfs/gors-compile /usr/local/bin/gors-compile
COPY --chmod=755 rootfs/gors-run /usr/local/bin/gors-run
COPY --chmod=755 rootfs/gors-runtime-publish /usr/local/bin/gors-runtime-publish
COPY --chmod=755 rootfs/gors-runtime-verify /usr/local/bin/gors-runtime-verify
COPY --chmod=755 rootfs/gors-warmup /usr/local/bin/gors-warmup
COPY rootfs/rustc-flags /usr/local/share/gors/rustc-flags
COPY rootfs/warmup.rs /usr/local/share/gors/warmup.rs

RUN for svc in devfs dmesg mdev hwdrivers; do rc-update add $svc sysinit; done && \
    for svc in hwclock modules sysctl hostname syslog bootmisc; do rc-update add $svc boot; done

# Verify the final image's live rustc, canonical producer record, schema-2
# manifest, exact rlib payload, and external link before publishing rootfs.
RUN /usr/local/bin/gors-runtime-publish && \
    /usr/local/bin/gors-runtime-verify && \
    /usr/local/bin/gors-warmup --smoke

RUN printf '%s\n' \
      '::once:/usr/local/bin/gors-warmup > /dev/ttyS0' \
      'ttyS0::respawn:/bin/sh' \
      > /etc/inittab

RUN rm -rf /usr/share/man /usr/share/doc /usr/share/info /usr/share/mkinitfs \
           /usr/share/licenses /usr/include /var/cache/apk/* /tmp/* \
           /usr/lib/rustlib/src /usr/lib/rustlib/etc \
           /usr/lib/rustlib/*/lib/libtest-*.rlib \
           /usr/lib/rustlib/*/lib/libproc_macro-*.rlib \
           /usr/libexec/gcc/*/*/cc1 /usr/libexec/gcc/*/*/cc1plus \
           /usr/libexec/gcc/*/*/lto1 /usr/libexec/gcc/*/*/f951 \
           /usr/libexec/gcc/*/*/plugin /usr/libexec/rust-analyzer-proc-macro-srv \
           /usr/bin/rustdoc /usr/bin/rust-gdb /usr/bin/rust-gdbgui /usr/bin/rust-lldb

RUN find /usr/lib/rustlib -name '*.so' -exec strip --strip-unneeded {} + 2>/dev/null; \
    strip --strip-unneeded /usr/bin/rustc 2>/dev/null; \
    strip --strip-unneeded /usr/local/libexec/gors-runtime-provider-manifest 2>/dev/null; \
    strip --strip-unneeded /usr/local/libexec/gors-runtime-dependency-validator 2>/dev/null; \
    true

# Trimming changes the canonical target-libdir inventory. Publish the one and
# only provider identity again from the final filesystem, then prove it links.
RUN /usr/local/bin/gors-runtime-publish && \
    /usr/local/bin/gors-runtime-verify && \
    /usr/local/bin/gors-warmup --smoke && \
    rm -f /usr/local/bin/gors-runtime-publish

ENTRYPOINT ["/bin/sh"]
