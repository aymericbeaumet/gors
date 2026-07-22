//! Verified Rust representation IR.

mod dataflow;
mod effects;
mod model;
mod plans;
mod verify;

pub use crate::compiler::ids::{BasicBlockId, DefId, LocalId};
pub use model::{
    BasicBlock, BinaryOp, CallTarget, Constant, ControlFlowPlan, Effects, EntrypointPlan, File,
    Function, FunctionArtifactPlan, LocalDecl, Operand, PanicEdge, Place, PrintStep, Provenance,
    ReadOp, RustLinkage, RustSymbol, RustType, Rvalue, RvalueKind, Signature, SlotInitialization,
    Statement, StorageClass, StoreOp, SyntheticOrigin, Terminator, TerminatorKind, UnaryOp,
};

use crate::compiler::Diagnostic;

pub(in crate::compiler) use dataflow::select_read_operations;
pub(in crate::compiler) use effects::{
    panic_edge, rvalue_effects, statement_effects, terminator_effects,
};
pub(in crate::compiler) use plans::print_plan;

pub(super) fn verify(file: &File) -> Result<(), Diagnostic> {
    file.verify()
}

pub(super) fn verify_function(
    function: &Function,
    signatures: &std::collections::BTreeMap<DefId, Signature>,
) -> Result<(), Diagnostic> {
    verify::verify_function(function, signatures)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
