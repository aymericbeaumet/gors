# Threaded Wasm compiler preview

Production uses `www/gors-wasm-loader.ts` and the stable, single-threaded Wasm
artifact. GitHub Pages does not provide the cross-origin isolation headers that
shared-memory Wasm requires, so this fallback must remain deployable.

From the repository root, install the fixed toolchain components once, then
start the compiler-only preview:

```sh
rustup component add rust-src --toolchain nightly-2026-07-01
rustup target add wasm32-unknown-unknown --toolchain nightly-2026-07-01
npm --prefix www run serve:compiler-preview:threads
```

This uses the fixed `nightly-2026-07-01` toolchain, rebuilds `std` with atomics,
exports the linker-owned heap and TLS globals needed by wasm-bindgen's thread
transform, emits a separate `wasm/pkg-threads` package through wasm-pack's
`web` target, and serves:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`

The loader rejects non-isolated contexts, initializes the
`wasm-bindgen-rayon` pool before exposing the compiler, and verifies the pool
size. It requests
`max(1, min(4, navigator.hardwareConcurrency - 1))` Rayon workers, using a
hardware count of two when the browser does not report one. This leaves a
logical CPU for the controller and UI when possible while avoiding shared-memory
and resolver-lock contention on high-core-count browsers.

Webpack selects this loader only when `GORS_WASM_THREADS=1`. The worker
protocol, resolver cache validation, output cache, source maps, and compile API
remain identical to production. Without that environment variable, all normal
build and deploy commands continue to bundle the stable single-threaded
artifact.

The stable and threaded compilers share a resolver ABI fingerprint that excludes
only their operational scheduling features. The production resolver seed is
therefore imported by the threaded preview after the same strict source, target,
dependency, SDK, schema, and integrity validation. A standalone threaded-only
build still writes a tiny invalid placeholder when no stable seed has been
prepared, solely so Webpack can resolve the optional asset; the worker treats
that placeholder as a cache miss.

The threaded compiler uses Send-safe source/string boundaries around parallel
work. It never carries `syn` nodes between Wasm workers and does not contain
Rust reimplementations of Go standard-library behavior.

## CI and deployment contract

CI builds the stable `www/wasm` package and its deterministic resolver seed
once, caches the nested `www/wasm/target` workspace plus the validated seed,
and shares the resulting artifact with the production bundle and browser
suites. The pinned-nightly job separately builds `pkg-threads`, confirms that
the Rayon pool initializes in a cross-origin-isolated Chromium page, and
byte-compares compiler output for single-package and independent-stdlib-package
programs against that exact stable artifact. A worker-count change must remain
operational only; it cannot change generated Rust.

GitHub Pages remains the production host and cannot attach the COOP/COEP
response headers required by shared-memory Wasm. Therefore CI gates the
threaded artifact, but the Pages deployment deliberately continues to ship the
stable single-threaded loader. Enabling threads in production requires a host
or edge layer that can set those headers on the document and every embedded
resource.
