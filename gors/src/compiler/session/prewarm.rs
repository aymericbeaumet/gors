//! Revision-scoped fan-out of independent function query roots.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::CompilerSession;
use crate::compiler::CompilerError;
use crate::compiler::ids::{DefId, FileId};
use crate::compiler::session::readiness::RustIrRoot;

impl CompilerSession {
    pub(super) fn prewarm_rust_ir(&self, roots: &[RustIrRoot]) -> Result<(), CompilerError> {
        let mut roots = roots
            .iter()
            .map(|root| (root.file(), root.definition()))
            .collect::<Vec<_>>();
        roots.sort();
        if roots.is_empty() {
            return Ok(());
        }

        let scheduler = self.host.scheduler();
        let workers = scheduler.workers_for(roots.len());
        scheduler.record_wave(roots.len(), workers);
        if workers == 1 {
            for (file, definition) in roots {
                let outcome = salsa::Cancelled::catch(AssertUnwindSafe(|| {
                    self.database.verified_rust_ir(file, definition)
                }));
                if outcome.is_err() {
                    return Err(cancelled_wave_error());
                }
            }
            return Ok(());
        }

        let roots: Arc<[(FileId, DefId)]> = roots.into();
        let next = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        let snapshots = (0..workers)
            .map(|_| self.database.snapshot())
            .collect::<Vec<_>>();
        let outcomes = scheduler
            .run_ordered(snapshots, {
                let roots = Arc::clone(&roots);
                let next = Arc::clone(&next);
                let cancelled = Arc::clone(&cancelled);
                move |snapshot| {
                    while !cancelled.load(Ordering::Acquire) {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((file, definition)) = roots.get(index).copied() else {
                            break;
                        };
                        let outcome = salsa::Cancelled::catch(AssertUnwindSafe(|| {
                            snapshot.verified_rust_ir(file, definition)
                        }));
                        if outcome.is_err() {
                            cancelled.store(true, Ordering::Release);
                            break;
                        }
                    }
                    cancelled.load(Ordering::Acquire)
                }
            })
            .map_err(|error| {
                CompilerError::backend(format!("compiler worker pool failed: {error}"))
            })?;

        // `run_ordered` joins every task and consumes every database snapshot.
        // It is now safe for a later revision to mutate Salsa inputs.
        if outcomes.into_iter().any(|cancelled| cancelled) {
            return Err(cancelled_wave_error());
        }
        Ok(())
    }
}

fn cancelled_wave_error() -> CompilerError {
    CompilerError::backend("compiler query wave was cancelled before artifact publication")
}
