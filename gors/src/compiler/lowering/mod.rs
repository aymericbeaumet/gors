//! Mandatory Rust representation lowering.

mod lower;

use crate::compiler::Diagnostic;
use crate::compiler::VerifiedMir;
use crate::compiler::rust_ir;

pub(super) fn lower(input: VerifiedMir) -> Result<rust_ir::File, Vec<Diagnostic>> {
    let normalized = crate::compiler::mir::normalize(input)?;
    lower::lower_file(normalized.into_inner()).map_err(|diagnostic| vec![diagnostic])
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
