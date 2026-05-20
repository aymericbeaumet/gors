FROM alpine:edge AS builder

RUN apk add --no-cache curl linux-virt linux-firmware-none rust

RUN mkinitfs -F "base virtio 9p" $(cat /usr/share/kernel/virt/kernel.release)

RUN echo 'fn main() { println!("ok"); }' > /tmp/t.rs && \
    rustc --edition 2024 -o /tmp/t /tmp/t.rs && /tmp/t && rm /tmp/t /tmp/t.rs

FROM alpine:edge

RUN apk add --no-cache openrc alpine-base linux-virt linux-firmware-none rust

COPY --from=builder /boot/initramfs-virt /boot/initramfs-virt

RUN for svc in devfs dmesg mdev hwdrivers; do rc-update add $svc sysinit; done && \
    for svc in hwclock modules sysctl hostname syslog bootmisc; do rc-update add $svc boot; done

RUN printf '%s\n' \
      '::once:/bin/echo GORS_BOOT_READY > /dev/ttyS0' \
      'ttyS0::respawn:/bin/sh' \
      > /etc/inittab

# compile-run <id>
# Input:  /tmp/<id>.rs (written by JS via 9p)
# Output: /tmp/<id>.out (stdout) and /tmp/<id>.err (stderr or compile error)
# Signal: GORS_DONE:<id>:ok:<exit_code> or GORS_DONE:<id>:compile_error
COPY <<'SCRIPT' /usr/local/bin/compile-run
#!/bin/sh
ID="$1"
cd /tmp
ln -sf "${ID}.rs" main.rs
rustc --edition 2024 -C opt-level=0 -C debuginfo=0 -C codegen-units=256 \
  -o "${ID}.bin" main.rs 2>"${ID}.err"
if [ $? -ne 0 ]; then
  printf 'GORS_DONE:%s:compile_error\n' "$ID"
  exit 0
fi
"${ID}.bin" >"${ID}.out" 2>"${ID}.err"
printf 'GORS_DONE:%s:ok:%s\n' "$ID" "$?"
SCRIPT
RUN chmod +x /usr/local/bin/compile-run

RUN rm -rf /usr/share/man /usr/share/doc /usr/share/info /usr/share/mkinitfs \
           /usr/include /var/cache/apk/* /tmp/* \
           /usr/lib/rustlib/src /usr/lib/rustlib/etc \
           /usr/libexec/gcc/*/15.*/cc1 /usr/libexec/gcc/*/15.*/cc1plus \
           /usr/libexec/gcc/*/15.*/lto1 /usr/libexec/gcc/*/15.*/f951 \
           /usr/libexec/gcc/*/15.*/plugin /usr/libexec/rust-analyzer-proc-macro-srv

RUN rustc --version

ENTRYPOINT ["/bin/sh"]
