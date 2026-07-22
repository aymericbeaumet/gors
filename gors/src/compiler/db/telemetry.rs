//! Query execution and engine-event telemetry.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// Compiler-owned query categories exposed to tests and performance tooling.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
    /// Build the package's body-independent exact typed signature index.
    PackageSignatures,
    /// Lower and verify one stable definition's explicit-order Go MIR.
    VerifiedGoMir,
    /// Normalize and reverify one stable definition's Go MIR.
    NormalizedGoMir,
    /// Select and verify one stable definition's Rust representation.
    VerifiedRustIr,
    /// Assemble and reverify a complete Rust IR package.
    RustIrPackage,
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
        self.executions.values().copied().sum()
    }

    /// Coarse events observed directly from the Salsa engine.
    #[must_use]
    pub const fn engine(&self) -> EngineEventCounts {
        self.engine
    }
}

#[derive(Default)]
pub(super) struct Telemetry {
    state: Mutex<TelemetrySnapshot>,
}

impl Telemetry {
    pub(super) fn record_query(&self, kind: QueryKind) {
        self.with_state(|state| {
            let count = state.executions.entry(kind).or_default();
            *count = count.saturating_add(1);
        });
    }

    pub(super) fn record_event(&self, event: &salsa::EventKind) {
        self.with_state(|state| match event {
            salsa::EventKind::WillExecute { .. } => {
                state.engine.will_execute = state.engine.will_execute.saturating_add(1);
            }
            salsa::EventKind::DidValidateMemoizedValue { .. } => {
                state.engine.did_validate = state.engine.did_validate.saturating_add(1);
            }
            salsa::EventKind::DidDiscard { .. } => {
                state.engine.did_discard = state.engine.did_discard.saturating_add(1);
            }
            salsa::EventKind::WillCheckCancellation => {
                state.engine.cancellation_checks =
                    state.engine.cancellation_checks.saturating_add(1);
            }
            _ => {}
        });
    }

    pub(super) fn snapshot(&self) -> TelemetrySnapshot {
        self.with_state(|state| state.clone())
    }

    pub(super) fn reset(&self) {
        self.with_state(|state| *state = TelemetrySnapshot::default());
    }

    fn with_state<T>(&self, operation: impl FnOnce(&mut TelemetrySnapshot) -> T) -> T {
        match self.state.lock() {
            Ok(mut state) => operation(&mut state),
            Err(poisoned) => operation(&mut poisoned.into_inner()),
        }
    }
}
