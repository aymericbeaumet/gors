## Known-failing fixture (skipped via `_` prefix)

Skipped preemptively alongside `crypto/_sha1` — same fixture shape,
same underlying lowering bug in the `crypto/shaN` package translation.
See `crypto/_sha1/NOTES.md`.
