FROM --platform=linux/386 alpine:edge AS builder

RUN apk add --no-cache curl linux-virt linux-firmware-none rust

RUN mkinitfs -F "base virtio 9p" $(cat /usr/share/kernel/virt/kernel.release)

FROM --platform=linux/386 alpine:edge
ARG RUST_VERSION=1.96.0

RUN apk add --no-cache openrc alpine-base linux-virt linux-firmware-none rust mold

COPY --from=builder /boot/initramfs-virt /boot/initramfs-virt

RUN for svc in devfs dmesg mdev hwdrivers; do rc-update add $svc sysinit; done && \
    for svc in hwclock modules sysctl hostname syslog bootmisc; do rc-update add $svc boot; done

# Shared rustc flags: fast compile, no debug info, mold linker, incremental cache
COPY <<'SCRIPT' /usr/local/bin/gors-rustc-flags
-C opt-level=0 -C debuginfo=0 -C codegen-units=16 -C link-arg=-fuse-ld=mold -C incremental=/tmp/rustc-incr
SCRIPT

# gors-compile <id>
# Input:  /tmp/<id>.rs
# Output: /tmp/<id>.bin, /tmp/<id>.compile.status ("0" on success, non-zero on error),
#         /tmp/<id>.compile.err (stderr)
# Signal: GORS_COMPILE_DONE:<id>  (pure trigger, all data in files)
COPY <<'SCRIPT' /usr/local/bin/gors-compile
#!/bin/sh
ID="$1"
FLAGS=$(cat /usr/local/bin/gors-rustc-flags)
cd /tmp
ln -sf "${ID}.rs" main.rs
rustc --edition 2024 $FLAGS \
  -o "${ID}.bin" main.rs 2>"${ID}.compile.err"
printf '%s' "$?" >"${ID}.compile.status"
printf 'GORS_COMPILE_DONE:%s\n' "$ID"
SCRIPT
RUN chmod +x /usr/local/bin/gors-compile

# gors-run <id>
# Input:  /tmp/<id>.bin
# Output: /tmp/<id>.run.status (exit code), /tmp/<id>.run.out (stdout),
#         /tmp/<id>.run.err (stderr)
# Signal: GORS_RUN_DONE:<id>  (pure trigger, all data in files)
COPY <<'SCRIPT' /usr/local/bin/gors-run
#!/bin/sh
ID="$1"
cd /tmp
"./${ID}.bin" >"${ID}.run.out" 2>"${ID}.run.err"
printf '%s' "$?" >"${ID}.run.status"
printf 'GORS_RUN_DONE:%s\n' "$ID"
SCRIPT
RUN chmod +x /usr/local/bin/gors-run

# Warmup program exercising the types gors generates — used both at build time
# (validation) and at boot time (priming incremental cache before state save).
COPY <<'WARMUP' /usr/local/share/gors/warmup.rs
#![allow(non_snake_case)]
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Condvar};
use std::fmt;
fn main() {
    let _ = HashMap::<String, i64>::new();
    let _ = Arc::new((Mutex::new(0), Condvar::new()));
    let _ = Vec::<i64>::new();
    let _ = format!("{}", "hello");
    println!("ok");
}
WARMUP

# Validate rustc works at build time
RUN rustc --edition 2024 -C opt-level=0 -C debuginfo=0 -C codegen-units=16 \
      -o /tmp/warmup /usr/local/share/gors/warmup.rs && /tmp/warmup && rm /tmp/warmup

# gors-warmup: compiles warmup program with incremental flags to prime the cache.
# Runs at boot time so the IndexedDB state snapshot includes the warm cache.
COPY <<'SCRIPT' /usr/local/bin/gors-warmup
#!/bin/sh
FLAGS=$(cat /usr/local/bin/gors-rustc-flags)
cd /tmp
cp /usr/local/share/gors/warmup.rs main.rs
rustc --edition 2024 $FLAGS -o /tmp/warmup main.rs 2>/dev/null
rm -f /tmp/warmup /tmp/main.rs
echo GORS_BOOT_READY
SCRIPT
RUN chmod +x /usr/local/bin/gors-warmup

RUN printf '%s\n' \
      '::once:/usr/local/bin/gors-warmup > /dev/ttyS0' \
      'ttyS0::respawn:/bin/sh' \
      > /etc/inittab

RUN rm -rf /usr/share/man /usr/share/doc /usr/share/info /usr/share/mkinitfs \
           /usr/share/licenses /usr/include /var/cache/apk/* /tmp/* \
           /usr/lib/rustlib/src /usr/lib/rustlib/etc \
           /usr/lib/rustlib/*/lib/libtest-*.rlib \
           /usr/lib/rustlib/*/lib/libproc_macro-*.rlib \
           /usr/libexec/gcc/*/15.*/cc1 /usr/libexec/gcc/*/15.*/cc1plus \
           /usr/libexec/gcc/*/15.*/lto1 /usr/libexec/gcc/*/15.*/f951 \
           /usr/libexec/gcc/*/15.*/plugin /usr/libexec/rust-analyzer-proc-macro-srv \
           /usr/bin/rustdoc /usr/bin/rust-gdb /usr/bin/rust-gdbgui /usr/bin/rust-lldb

RUN find /usr/lib/rustlib -name '*.so' -exec strip --strip-unneeded {} + 2>/dev/null; \
    strip --strip-unneeded /usr/bin/rustc 2>/dev/null; \
    true

RUN rustc --version | grep "rustc ${RUST_VERSION} "

ENTRYPOINT ["/bin/sh"]
