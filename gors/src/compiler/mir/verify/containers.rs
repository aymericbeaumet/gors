//! MIR verification helpers for slice and map runtime operations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
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

pub(super) fn verify_bool_slice_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    let slice = Ty::Slice(Box::new(Ty::Bool));
    let (expected, results) = match builtin {
        hir::Builtin::SliceBoolIndex => (vec![slice, Ty::Int(IntTy::Int)], vec![Ty::Bool]),
        hir::Builtin::SliceBoolSet => (vec![slice, Ty::Int(IntTy::Int), Ty::Bool], Vec::new()),
        _ => {
            return Err(Diagnostic::backend(
                "bool slice verifier received a non-bool-slice operation",
            ));
        }
    };
    if arguments != expected {
        return Err(Diagnostic::backend(format!(
            "invalid MIR bool slice arguments: {arguments:?}"
        )));
    }
    Ok(results)
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
