//! Mandatory Rust representation lowering.

mod lower;

use crate::compiler::Diagnostic;
#[cfg(test)]
use crate::compiler::VerifiedMir;
use crate::compiler::mir;
use crate::compiler::rust_ir;
use crate::compiler::types;

pub(super) fn lower_signature(
    signature: &types::Signature,
) -> Result<rust_ir::Signature, Diagnostic> {
    lower::lower_signature(signature)
}

pub(super) fn lower_function(
    function: mir::Function,
    executable_package: bool,
) -> Result<rust_ir::Function, Diagnostic> {
    lower::lower_function(function, executable_package)
}

#[cfg(test)]
pub(super) fn lower(input: VerifiedMir) -> Result<rust_ir::File, Vec<Diagnostic>> {
    let normalized = crate::compiler::mir::normalize(input)?;
    lower::lower_file(normalized.into_inner()).map_err(|diagnostic| vec![diagnostic])
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
