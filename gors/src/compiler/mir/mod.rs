//! Go-semantic, evaluation-order-explicit MIR.

mod construct;
mod dataflow;
mod lower;
mod model;
mod normalize;
mod verify;

pub use model::{
    BasicBlock, File, Function, LocalDecl, Operand, PanicEdge, Place, Provenance, Rvalue,
    RvalueKind, Statement, SyntheticOrigin, Terminator, TerminatorKind,
};

use crate::compiler::Diagnostic;
use crate::compiler::VerifiedMir;
use crate::compiler::hir;

pub(super) fn lower_file(file: &hir::File) -> Result<File, Vec<Diagnostic>> {
    lower::lower_file(file)
}

pub(super) fn verify(file: &File) -> Result<(), Diagnostic> {
    file.verify()
}

pub(super) fn normalize(input: VerifiedMir) -> Result<VerifiedMir, Vec<Diagnostic>> {
    normalize::normalize(input)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests;
