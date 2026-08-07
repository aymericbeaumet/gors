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
    FunctionArtifactPlan, LocalDecl, Operand, PanicCleanup, PanicEdge, PanicPayloadCapture,
    PanicPayloadRethrow, Place, Provenance, ReadOp, RustLinkage, RustSymbol, RustType, Rvalue,
    RvalueKind, Signature, SlotInitialization, Statement, StorageClass, StoreOp, SyntheticOrigin,
    Terminator, TerminatorKind, ValueOp,
};

use crate::compiler::Diagnostic;

pub(in crate::compiler) use dataflow::select_read_operations;
pub(in crate::compiler) use effects::{
    runtime_effects, rvalue_effects, statement_effects, terminator_effects,
};
pub(in crate::compiler) use idiom::select_control_flow_plan;
#[cfg(test)]
pub(super) fn verify(file: &File) -> Result<RuntimeRequirement, Diagnostic> {
    file.verify()
}

pub(super) fn verify_function(
    function: &Function,
    owner: crate::compiler::ids::QualifiedDefId,
    signatures: &std::collections::BTreeMap<crate::compiler::ids::QualifiedDefId, Signature>,
) -> Result<RuntimeRequirement, Diagnostic> {
    verify::verify_function(function, owner, signatures)
}

pub(super) fn verify_with_signatures(
    file: &File,
    signatures: &std::collections::BTreeMap<crate::compiler::ids::QualifiedDefId, Signature>,
) -> Result<RuntimeRequirement, Diagnostic> {
    file.verify_with_signatures(signatures)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
