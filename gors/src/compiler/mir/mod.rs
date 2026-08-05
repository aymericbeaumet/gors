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

use std::collections::BTreeMap;

use crate::compiler::Diagnostic;
#[cfg(test)]
use crate::compiler::VerifiedMir;
use crate::compiler::hir;

#[cfg(test)]
pub(super) fn lower_file(file: &hir::File) -> Result<File, Vec<Diagnostic>> {
    lower::lower_file(file)
}

pub(super) fn lower_function(function: &hir::Function) -> Result<Function, Diagnostic> {
    lower::lower_function(function)
}

#[cfg(test)]
pub(super) fn verify(file: &File) -> Result<(), Diagnostic> {
    file.verify()
}

pub(super) type SignatureIndex =
    BTreeMap<crate::compiler::ids::DefId, crate::compiler::types::Signature>;

pub(super) fn verify_function(
    function: &Function,
    signatures: &SignatureIndex,
) -> Result<(), Diagnostic> {
    function.verify_with_signatures(signatures)
}

#[cfg(test)]
pub(super) fn normalize(input: VerifiedMir) -> Result<VerifiedMir, Vec<Diagnostic>> {
    normalize::normalize(input)
}

pub(super) fn normalize_function(
    function: Function,
    signatures: &SignatureIndex,
) -> Result<Function, Vec<Diagnostic>> {
    normalize::normalize_function(function, signatures)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests;
