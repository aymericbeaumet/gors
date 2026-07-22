//! Native compiler worker-pool policy.
//!
//! Go packages can contain deeply nested declarations and expressions, and the
//! parser/compiler passes intentionally recurse through those source shapes.
//! Rayon otherwise inherits Rust's comparatively small spawned-thread stack,
//! making the same valid package compile sequentially but abort in a parallel
//! package, file, type-environment, or local-package worker.

#[cfg(all(feature = "parallel", not(target_family = "wasm")))]
pub(crate) const COMPILER_WORKER_STACK_SIZE_BYTES: usize = 16 * 1024 * 1024;

#[cfg(all(feature = "parallel", not(target_family = "wasm")))]
pub(crate) fn builder(thread_count: usize, worker_kind: &'static str) -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .stack_size(COMPILER_WORKER_STACK_SIZE_BYTES)
        .thread_name(move |index| format!("gors-{worker_kind}-{index}"))
}

#[cfg(all(test, feature = "parallel", not(target_family = "wasm")))]
mod tests {
    use super::*;

    // Keep each frame live across the recursive call so this proves the pool's
    // configured stack, rather than an optimizer turning the probe into a loop.
    #[inline(never)]
    fn recursive_compiler_stack_probe(depth: usize) -> usize {
        let padding = [depth as u8; 4096];
        std::hint::black_box(&padding);
        let nested = if depth == 0 {
            0
        } else {
            recursive_compiler_stack_probe(depth - 1)
        };
        nested + usize::from(std::hint::black_box(padding[depth % padding.len()]))
    }

    #[test]
    fn configured_worker_supports_deep_recursive_compiler_workloads() {
        let pool = builder(1, "stack-probe").build().unwrap();
        let (thread_name, checksum) = pool.install(|| {
            (
                std::thread::current()
                    .name()
                    .unwrap_or_default()
                    .to_string(),
                recursive_compiler_stack_probe(1024),
            )
        });

        assert_eq!(thread_name, "gors-stack-probe-0");
        assert!(checksum > 0);
    }
}
