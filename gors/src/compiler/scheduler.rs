//! Compiler-owned native job admission and worker-pool policy.
//!
//! Queries never submit nested work. A session may fan out independent query
//! roots through its host, and every wave joins before the session can mutate
//! Salsa inputs for the next revision.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[cfg(not(target_family = "wasm"))]
use std::sync::Mutex;

#[cfg(not(target_family = "wasm"))]
use rayon::prelude::*;

use super::CompilerError;
use super::db::BuildConfig;
use super::session::CompilerSession;

const MIN_PARALLEL_TASKS: usize = 8;

#[cfg(not(target_family = "wasm"))]
// Lowering is still recursive, so keep a bounded compiler stack policy.
// Deep recursion should move to explicit worklists, not larger stacks.
const COMPILER_WORKER_STACK_SIZE_BYTES: usize = 4 * 1024 * 1024;

/// One explicit compiler-wide job budget and its lazily created native pool.
///
/// Clone the host to share the same admission budget and worker pool across
/// retained sessions. It contains no semantic state and never owns a Salsa
/// snapshot, so source revisions remain independently mutable and evictable.
#[derive(Clone)]
pub struct CompilerHost {
    scheduler: Arc<CompilerScheduler>,
}

impl CompilerHost {
    /// Create a host with an exact positive maximum number of compiler jobs.
    pub fn new(job_budget: NonZeroUsize) -> Result<Self, CompilerError> {
        #[cfg(target_family = "wasm")]
        if job_budget != NonZeroUsize::MIN {
            return Err(CompilerError::backend(
                "the Wasm compiler supports exactly one inline job",
            ));
        }

        Ok(Self {
            scheduler: Arc::new(CompilerScheduler::new(job_budget)),
        })
    }

    /// Create an explicitly inline host that never starts native workers.
    #[must_use]
    pub fn inline() -> Self {
        Self {
            scheduler: Arc::new(CompilerScheduler::new(NonZeroUsize::MIN)),
        }
    }

    /// Create one retained semantic session sharing this host's job budget.
    pub fn session(&self, config: BuildConfig) -> Result<CompilerSession, CompilerError> {
        CompilerSession::from_host(config, self.clone())
    }

    /// Exact maximum number of admitted jobs.
    #[must_use]
    pub fn job_budget(&self) -> NonZeroUsize {
        self.scheduler.job_budget()
    }

    /// Non-semantic scheduler evidence for tests and performance traces.
    #[must_use]
    pub fn telemetry(&self) -> SchedulerTelemetry {
        self.scheduler.telemetry()
    }

    pub(super) fn scheduler(&self) -> &CompilerScheduler {
        &self.scheduler
    }
}

/// Immutable observation of scheduler admission, not semantic query state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SchedulerTelemetry {
    /// Number of serial query-root waves.
    pub serial_waves: u64,
    /// Number of native parallel query-root waves.
    pub parallel_waves: u64,
    /// Total stable function roots offered to the scheduler.
    pub scheduled_roots: u64,
    /// Number of revision-scoped Salsa snapshots created for workers.
    pub snapshots_created: u64,
    /// Native pool constructions (at most once per host in normal operation).
    pub pool_starts: u64,
    /// Largest number of workers admitted to one wave.
    pub peak_workers: usize,
}

#[derive(Default)]
struct SchedulerCounters {
    serial_waves: AtomicU64,
    parallel_waves: AtomicU64,
    scheduled_roots: AtomicU64,
    snapshots_created: AtomicU64,
    pool_starts: AtomicU64,
    peak_workers: AtomicUsize,
}

pub(super) struct CompilerScheduler {
    job_budget: NonZeroUsize,
    counters: SchedulerCounters,
    #[cfg(not(target_family = "wasm"))]
    pool: Mutex<Option<Arc<rayon::ThreadPool>>>,
}

impl CompilerScheduler {
    fn new(job_budget: NonZeroUsize) -> Self {
        Self {
            job_budget,
            counters: SchedulerCounters::default(),
            #[cfg(not(target_family = "wasm"))]
            pool: Mutex::new(None),
        }
    }

    const fn job_budget(&self) -> NonZeroUsize {
        self.job_budget
    }

    /// Number of worker contexts to construct for one independent root set.
    ///
    /// Small waves stay inline so short-lived convenience compilations do not
    /// pay native thread startup. The first larger wave starts the host's one
    /// fixed-capacity pool, which every session sharing that host then reuses.
    pub(super) fn workers_for(&self, roots: usize) -> usize {
        if self.job_budget.get() == 1 || roots < MIN_PARALLEL_TASKS {
            1
        } else {
            self.job_budget.get().min(roots)
        }
    }

    pub(super) fn record_wave(&self, roots: usize, workers: usize) {
        self.counters
            .scheduled_roots
            .fetch_add(roots as u64, Ordering::Relaxed);
        if workers == 1 {
            self.counters.serial_waves.fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters.parallel_waves.fetch_add(1, Ordering::Relaxed);
            self.counters
                .snapshots_created
                .fetch_add(workers as u64, Ordering::Relaxed);
            self.counters
                .peak_workers
                .fetch_max(workers, Ordering::Relaxed);
        }
    }

    /// Run already-bounded worker contexts and preserve their input order.
    ///
    /// The caller owns every revision-scoped Salsa snapshot in `workers`; the
    /// returned vector proves all worker closures joined and their contexts
    /// were dropped before any later input mutation.
    #[cfg(not(target_family = "wasm"))]
    pub(super) fn run_ordered<T, R, F>(
        &self,
        workers: Vec<T>,
        operation: F,
    ) -> Result<Vec<R>, SchedulerError>
    where
        T: Send,
        R: Send,
        F: Fn(T) -> R + Send + Sync,
    {
        if workers.len() <= 1 {
            return Ok(workers.into_iter().map(operation).collect());
        }
        let pool = self.pool()?;
        Ok(pool.install(|| workers.into_par_iter().map(operation).collect()))
    }

    #[cfg(target_family = "wasm")]
    pub(super) fn run_ordered<T, R, F>(
        &self,
        workers: Vec<T>,
        operation: F,
    ) -> Result<Vec<R>, SchedulerError>
    where
        F: Fn(T) -> R,
    {
        Ok(workers.into_iter().map(operation).collect())
    }

    fn telemetry(&self) -> SchedulerTelemetry {
        SchedulerTelemetry {
            serial_waves: self.counters.serial_waves.load(Ordering::Relaxed),
            parallel_waves: self.counters.parallel_waves.load(Ordering::Relaxed),
            scheduled_roots: self.counters.scheduled_roots.load(Ordering::Relaxed),
            snapshots_created: self.counters.snapshots_created.load(Ordering::Relaxed),
            pool_starts: self.counters.pool_starts.load(Ordering::Relaxed),
            peak_workers: self.counters.peak_workers.load(Ordering::Relaxed),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn pool(&self) -> Result<Arc<rayon::ThreadPool>, SchedulerError> {
        let mut state = match self.pool.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(existing) = state.as_ref() {
            return Ok(Arc::clone(existing));
        }

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.job_budget.get())
            .stack_size(COMPILER_WORKER_STACK_SIZE_BYTES)
            .thread_name(|index| format!("gors-compiler-{index}"))
            .build()
            .map_err(|error| SchedulerError::new(error.to_string()))?;
        let pool = Arc::new(pool);
        *state = Some(Arc::clone(&pool));
        self.counters.pool_starts.fetch_add(1, Ordering::Relaxed);
        drop(state);
        Ok(pool)
    }
}

#[derive(Debug)]
pub(super) struct SchedulerError {
    message: String,
}

impl SchedulerError {
    #[cfg(not(target_family = "wasm"))]
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests;
