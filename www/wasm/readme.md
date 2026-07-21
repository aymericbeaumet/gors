# gors-wasm

## Build

From the repository root:

```sh
npm --prefix www run build:wasm
```

The production artifact is intentionally stable and single-threaded. For the
opt-in shared-memory build and isolated local preview, use:

```sh
npm --prefix www run build:wasm:threads
npm --prefix www run serve:compiler-preview:threads
```

See [threads-preview.md](threads-preview.md) for the fixed-nightly build
contract and production fallback boundary.

The browser worker persists the generic resolver cache exported by these
bindings as one byte-bounded IndexedDB snapshot. The Rust resolver validates
the schema, pinned Go SDK, stdlib version, and resolver ABI fingerprint before
accepting it; invalid snapshots are deleted and compilation continues as a
normal cache miss. That fingerprint excludes only the `parallel` and
`wasm-threads` scheduling features, so the deterministic stable seed is reusable
by the threaded preview while semantic compiler, target, dependency, or SDK
changes still invalidate it. The archive contains mechanically generated
compiler output, not handwritten Go standard-library implementations.

`npm --prefix www run build:resolver-seed` runs the already-built production
Wasm compiler against `www/default-playground.go`, exports that same generic
resolver/type-environment archive, and emits a capped, versioned first-load
asset. The worker prefers a valid IndexedDB snapshot and fetches this seed only
when no valid snapshot exists. The shipped archive is gzip-compressed and
decompressed through a byte-capped stream; browsers without that support
continue with an ordinary cache miss. The seed is regenerated whenever the
exact Wasm binary or default source changes.
