//! Canonical representation and Go-observable effects of runtime operations.

use crate::encoding::CanonicalEncoder;

/// Allocation behavior introduced by a runtime operation.
///
/// This records whether an operation can allocate or grow dynamic storage. It
/// deliberately does not classify deallocation or process-level allocation
/// failure as a Go-observable panic.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AllocationEffect {
    None,
    MayAllocate,
}

/// Mutation an operation may perform through an owned argument.
///
/// This makes representation-introduced mutation explicit even when Go sees
/// only a newly produced value, allowing Rust IR verification to account for
/// unique-buffer reuse without a compiler-local operation table.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArgumentMutationEffect {
    None,
    MayMutateOwnedArgument,
}

impl ArgumentMutationEffect {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::None => 1,
            Self::MayMutateOwnedArgument => 2,
        }
    }
}

impl AllocationEffect {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::None => 1,
            Self::MayAllocate => 2,
        }
    }
}

/// Host I/O performed by a runtime operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HostIoEffect {
    None,
    StandardError,
}

impl HostIoEffect {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::None => 1,
            Self::StandardError => 2,
        }
    }
}

/// A Go language panic condition exposed by a runtime operation.
///
/// Implementation faults, resource exhaustion, and host-process termination
/// are not Go panics and therefore do not appear in this catalog.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GoPanicCondition {
    IntegerDivideByZero,
    NegativeShiftAmount,
    ExplicitPanic,
}

impl GoPanicCondition {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::IntegerDivideByZero => 1,
            Self::NegativeShiftAmount => 2,
            Self::ExplicitPanic => 3,
        }
    }
}

/// Canonical representation and Go-observable effect summary for one operation.
///
/// The compiler still accounts for generic call, move, drop, and control-flow
/// effects around this operation; this descriptor replaces only
/// operation-specific effect knowledge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeEffects {
    allocation: AllocationEffect,
    argument_mutation: ArgumentMutationEffect,
    host_io: HostIoEffect,
    go_panics: &'static [GoPanicCondition],
}

impl RuntimeEffects {
    pub(crate) const fn new(
        allocation: AllocationEffect,
        argument_mutation: ArgumentMutationEffect,
        host_io: HostIoEffect,
        go_panics: &'static [GoPanicCondition],
    ) -> Self {
        Self {
            allocation,
            argument_mutation,
            host_io,
            go_panics,
        }
    }

    /// Whether the operation may allocate or grow dynamic storage.
    #[must_use]
    pub const fn allocation(self) -> AllocationEffect {
        self.allocation
    }

    /// Whether the operation may reuse and mutate owned argument storage.
    #[must_use]
    pub const fn argument_mutation(self) -> ArgumentMutationEffect {
        self.argument_mutation
    }

    /// Which host output surface the operation writes, if any.
    #[must_use]
    pub const fn host_io(self) -> HostIoEffect {
        self.host_io
    }

    /// Sorted Go language panic conditions exposed by the operation.
    #[must_use]
    pub const fn go_panics(self) -> &'static [GoPanicCondition] {
        self.go_panics
    }

    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u8(self.allocation.canonical_tag());
        encoder.u8(self.argument_mutation.canonical_tag());
        encoder.u8(self.host_io.canonical_tag());
        encoder.count(self.go_panics.len());
        for panic in self.go_panics {
            encoder.u8(panic.canonical_tag());
        }
    }
}
