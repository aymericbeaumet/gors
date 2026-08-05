//! Verified Rust representation IR.

mod dataflow;
mod effects;
mod idiom;
mod model;
mod verify;

pub use crate::compiler::ids::{BasicBlockId, DefId, LocalId};
pub use gors_runtime_abi::{PrimitiveOp, RuntimeOp, RuntimeRequirement};
pub use model::{
    BasicBlock, CallTarget, Constant, ControlFlowPlan, Effects, EntrypointPlan, File, Function,
    FunctionArtifactPlan, LocalDecl, Operand, PanicCleanup, PanicEdge, Place, Provenance, ReadOp,
    RustLinkage, RustSymbol, RustType, Rvalue, RvalueKind, Signature, SlotInitialization,
    Statement, StorageClass, StoreOp, SyntheticOrigin, Terminator, TerminatorKind, ValueOp,
};

use crate::compiler::Diagnostic;

pub(in crate::compiler) use dataflow::select_read_operations;
pub(in crate::compiler) use effects::{rvalue_effects, statement_effects, terminator_effects};
pub(in crate::compiler) use idiom::select_control_flow_plan;
pub(super) fn verify(file: &File) -> Result<RuntimeRequirement, Diagnostic> {
    file.verify()
}

pub(super) fn verify_function(
    function: &Function,
    signatures: &std::collections::BTreeMap<DefId, Signature>,
) -> Result<RuntimeRequirement, Diagnostic> {
    verify::verify_function(function, signatures)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
