//! MIR verification helpers for slice and map runtime operations.

use crate::compiler::Diagnostic;
use crate::compiler::types::{IntTy, Ty, UintTy};

pub(super) fn verify_slice_call_arguments(
    arguments: &[Ty],
    expected_len: usize,
    context: &str,
) -> Result<(), Diagnostic> {
    let slice = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
    if arguments.len() != expected_len
        || arguments.first() != Some(&slice)
        || arguments
            .get(1..)
            .unwrap_or_default()
            .iter()
            .any(|ty| ty != &Ty::Int(IntTy::Int))
    {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} argument types: {arguments:?}"
        )));
    }
    Ok(())
}

pub(super) fn verify_byte_slice_call_arguments(
    arguments: &[Ty],
    second: &Ty,
    context: &str,
) -> Result<(), Diagnostic> {
    let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
    if arguments != [byte_slice, second.clone()] {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} argument types: {arguments:?}"
        )));
    }
    Ok(())
}

pub(super) fn map_string_i64_ty() -> Ty {
    Ty::Map(Box::new(Ty::String), Box::new(Ty::Int(IntTy::Int)))
}

pub(super) fn verify_map_call_arguments(
    arguments: &[Ty],
    expected: &[Ty],
    context: &str,
) -> Result<(), Diagnostic> {
    if arguments != expected {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} argument types: {arguments:?}"
        )));
    }
    Ok(())
}
