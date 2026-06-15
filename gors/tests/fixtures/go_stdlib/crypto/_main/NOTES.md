## Known-failing fixture (skipped via `_` prefix)

`rustc` fails to compile the generated `main.rs` (exit status 1). The
fixture exercises the root `crypto` package's `hash.Hash`-shaped
interface against a user-defined `fakeHash`, so the lowering issue is
somewhere in the `crypto` package translation or the surrounding
interface plumbing.

The sibling `crypto/*` subpackages still run, which is why this fixture
sits under `crypto/_main` rather than renaming `crypto` itself.

To re-enable: capture the actual `rustc` diagnostic, fix the
underlying lowering, then drop the `_main` indirection.
