#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use super::CompilerScheduler;

#[test]
fn small_waves_stay_inline() {
    let scheduler = CompilerScheduler::new(NonZeroUsize::new(4).unwrap());
    assert_eq!(scheduler.workers_for(7), 1);
    let output = scheduler.run_ordered(vec![3], |value| value * 2).unwrap();
    assert_eq!(output, [6]);
    assert_eq!(scheduler.telemetry().pool_starts, 0);
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn native_wave_is_bounded_and_preserves_result_order() {
    let scheduler = CompilerScheduler::new(NonZeroUsize::new(2).unwrap());
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let output = scheduler
        .run_ordered((0..8).collect(), |value| value)
        .unwrap();
    assert_eq!(output, (0..8).collect::<Vec<_>>());

    let barrier = Arc::new(Barrier::new(2));
    scheduler.pool().unwrap().broadcast({
        let active = Arc::clone(&active);
        let peak = Arc::clone(&peak);
        let barrier = Arc::clone(&barrier);
        move |_| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            barrier.wait();
            active.fetch_sub(1, Ordering::SeqCst);
        }
    });

    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn retained_scheduler_reuses_pool_capacity() {
    let scheduler = CompilerScheduler::new(NonZeroUsize::new(4).unwrap());
    let _ = scheduler
        .run_ordered((0..8).collect::<Vec<_>>(), |value| value)
        .unwrap();
    let first = scheduler.telemetry().pool_starts;
    let _ = scheduler
        .run_ordered((0..16).collect::<Vec<_>>(), |value| value)
        .unwrap();
    assert_eq!(scheduler.telemetry().pool_starts, first);
}
