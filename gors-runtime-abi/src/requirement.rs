//! Canonical per-program runtime operation requirements.

use crate::operations::RuntimeOp;
use crate::target::TargetCapabilities;

/// Sorted, duplicate-free runtime operations required by one compiled unit.
///
/// This is deliberately a semantic set rather than a bit mask: stable
/// [`RuntimeOp::id`] values define canonical order without coupling serialized
/// compiler products to Rust enum discriminants.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeRequirement {
    operations: Box<[RuntimeOp]>,
}

impl RuntimeRequirement {
    /// Canonicalize an arbitrary operation sequence into a compact set.
    #[must_use]
    pub fn new(operations: impl IntoIterator<Item = RuntimeOp>) -> Self {
        let mut operations: Vec<_> = operations.into_iter().collect();
        operations.sort_unstable_by_key(|operation| operation.id());
        operations.dedup_by_key(|operation| operation.id());
        Self {
            operations: operations.into_boxed_slice(),
        }
    }

    /// Return the canonical operation sequence.
    #[must_use]
    pub const fn as_slice(&self) -> &[RuntimeOp] {
        &self.operations
    }

    /// Iterate over operations in stable ID order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = RuntimeOp> + ExactSizeIterator + '_ {
        self.operations.iter().copied()
    }

    /// Whether this requirement contains an operation.
    #[must_use]
    pub fn contains(&self, operation: RuntimeOp) -> bool {
        self.operations
            .binary_search_by_key(&operation.id(), |candidate| candidate.id())
            .is_ok()
    }

    /// Combine two requirements without retaining order or duplicates.
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self::new(self.iter().chain(other.iter()))
    }

    /// Host facilities required by any selected runtime operation.
    #[must_use]
    pub fn required_capabilities(&self) -> TargetCapabilities {
        TargetCapabilities::new(
            self.iter()
                .flat_map(|operation| operation.required_capabilities().iter().copied()),
        )
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }

    pub(crate) fn encode(&self, encoder: &mut crate::encoding::CanonicalEncoder) {
        encoder.count(self.operations.len());
        for operation in &self.operations {
            encoder.u16(operation.id().get());
        }
    }
}

impl FromIterator<RuntimeOp> for RuntimeRequirement {
    fn from_iter<T: IntoIterator<Item = RuntimeOp>>(operations: T) -> Self {
        Self::new(operations)
    }
}

impl<'a> IntoIterator for &'a RuntimeRequirement {
    type Item = RuntimeOp;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, RuntimeOp>>;

    fn into_iter(self) -> Self::IntoIter {
        self.operations.iter().copied()
    }
}
