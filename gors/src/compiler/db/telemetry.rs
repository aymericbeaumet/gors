//! Query execution and engine-event telemetry.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Compiler-owned query categories exposed to tests and performance tooling.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(usize)]
pub enum QueryKind {
    /// Parse one immutable source snapshot and project its declarations.
    FileProjection,
    /// Type-check one independently parsed file into tracked HIR projections.
    SemanticFile,
    /// Materialize the body-independent file index.
    FileAnalysis,
    /// Merge sorted file projections into a body-independent package index.
    PackageAnalysis,
    /// Aggregate public function headers.
    PublicApi,
    /// Read one function's header projection.
    FunctionSignature,
    /// Read one function's body projection.
    FunctionBody,
    /// Read one function's revision-local source anchor.
    FunctionProvenance,
    /// Publish one stable definition's typed HIR.
    TypedHir,
    /// Publish one stable definition's exact typed signature.
    TypedSignature,
    /// Resolve one stable callee identity through the package index.
    PackageFunctionLookup,
    /// Build one function's self and direct-callee signature dependency set.
    SignatureDependencies,
    /// Classify one function's package as executable or library code.
    ExecutableRole,
    /// Lower and verify one stable definition's explicit-order Go MIR.
    VerifiedGoMir,
    /// Normalize and reverify one stable definition's Go MIR.
    NormalizedGoMir,
    /// Select and verify one stable definition's Rust representation.
    VerifiedRustIr,
    /// Assemble and reverify a complete Rust IR package.
    RustIrPackage,
}

impl QueryKind {
    const COUNT: usize = Self::RustIrPackage as usize + 1;
    const ALL: [Self; Self::COUNT] = [
        Self::FileProjection,
        Self::SemanticFile,
        Self::FileAnalysis,
        Self::PackageAnalysis,
        Self::PublicApi,
        Self::FunctionSignature,
        Self::FunctionBody,
        Self::FunctionProvenance,
        Self::TypedHir,
        Self::TypedSignature,
        Self::PackageFunctionLookup,
        Self::SignatureDependencies,
        Self::ExecutableRole,
        Self::VerifiedGoMir,
        Self::NormalizedGoMir,
        Self::VerifiedRustIr,
        Self::RustIrPackage,
    ];

    const fn index(self) -> usize {
        self as usize
    }
}

/// Coarse Salsa engine events, kept separate from compiler query counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EngineEventCounts {
    /// Query bodies Salsa elected to execute.
    pub will_execute: u64,
    /// Memoized values Salsa validated without executing their bodies.
    pub did_validate: u64,
    /// Memoized or tracked values discarded by the engine.
    pub did_discard: u64,
    /// Cancellation checks reached by active work.
    pub cancellation_checks: u64,
}

/// Immutable telemetry observation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TelemetrySnapshot {
    executions: BTreeMap<QueryKind, u64>,
    engine: EngineEventCounts,
}

impl TelemetrySnapshot {
    /// Number of times a compiler-owned query body executed.
    #[must_use]
    pub fn executions(&self, kind: QueryKind) -> u64 {
        self.executions.get(&kind).copied().unwrap_or(0)
    }

    /// Total compiler-owned query body executions.
    #[must_use]
    pub fn total_executions(&self) -> u64 {
        self.executions
            .values()
            .copied()
            .fold(0_u64, u64::saturating_add)
    }

    /// Coarse events observed directly from the Salsa engine.
    #[must_use]
    pub const fn engine(&self) -> EngineEventCounts {
        self.engine
    }
}

pub(super) struct Telemetry {
    executions: [AtomicU64; QueryKind::COUNT],
    will_execute: AtomicU64,
    did_validate: AtomicU64,
    did_discard: AtomicU64,
    cancellation_checks: AtomicU64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            executions: std::array::from_fn(|_| AtomicU64::new(0)),
            will_execute: AtomicU64::new(0),
            did_validate: AtomicU64::new(0),
            did_discard: AtomicU64::new(0),
            cancellation_checks: AtomicU64::new(0),
        }
    }
}

impl Telemetry {
    pub(super) fn record_query(&self, kind: QueryKind) {
        if let Some(counter) = self.executions.get(kind.index()) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn record_event(&self, event: &salsa::EventKind) {
        let counter = match event {
            salsa::EventKind::WillExecute { .. } => Some(&self.will_execute),
            salsa::EventKind::DidValidateMemoizedValue { .. } => Some(&self.did_validate),
            salsa::EventKind::DidDiscard { .. } => Some(&self.did_discard),
            salsa::EventKind::WillCheckCancellation => Some(&self.cancellation_checks),
            _ => None,
        };
        if let Some(counter) = counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn snapshot(&self) -> TelemetrySnapshot {
        let executions = QueryKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let executions = self
                    .executions
                    .get(kind.index())
                    .map_or(0, |counter| counter.load(Ordering::Relaxed));
                (executions != 0).then_some((kind, executions))
            })
            .collect();
        TelemetrySnapshot {
            executions,
            engine: EngineEventCounts {
                will_execute: self.will_execute.load(Ordering::Relaxed),
                did_validate: self.did_validate.load(Ordering::Relaxed),
                did_discard: self.did_discard.load(Ordering::Relaxed),
                cancellation_checks: self.cancellation_checks.load(Ordering::Relaxed),
            },
        }
    }

    pub(super) fn reset(&self) {
        for counter in &self.executions {
            counter.store(0, Ordering::Relaxed);
        }
        self.will_execute.store(0, Ordering::Relaxed);
        self.did_validate.store(0, Ordering::Relaxed);
        self.did_discard.store(0, Ordering::Relaxed);
        self.cancellation_checks.store(0, Ordering::Relaxed);
    }
}
