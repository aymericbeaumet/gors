## Known-failing fixture (skipped via `_` prefix)

`ch <- "first"` sends an &str literal into a `Chan<String>`. The generated
Rust calls `ch.send("first")` instead of `ch.send("first".to_string())`,
so rustc fails with E0308 (expected `String`, found `&str`). The lowering
needs to coerce string-literal sends to owned `String` when the channel
element type is `String`.
