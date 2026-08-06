//! MIR type and effect verification for fixed scalar arrays.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{FloatTy, IntTy, Ty};

pub(super) fn verify_array_literal(values: &[i64]) -> Result<(Ty, hir::Effects), Diagnostic> {
    let length = u64::try_from(values.len())
        .map_err(|_| Diagnostic::backend("MIR array literal length does not fit u64"))?;
    Ok((
        Ty::Array(length, Box::new(Ty::Int(IntTy::Int))),
        hir::Effects::default(),
    ))
}

pub(super) fn verify_scalar_array_literal(
    elements: Vec<Ty>,
    declared: &Ty,
) -> Result<(Ty, hir::Effects), Diagnostic> {
    let Ty::Array(length, element) = declared.underlying() else {
        return Err(Diagnostic::backend(format!(
            "MIR array literal has non-array type {declared:?}"
        )));
    };
    if usize::try_from(*length).ok() != Some(elements.len())
        || !is_scalar_element(element)
        || elements.iter().any(|actual| actual != element.as_ref())
    {
        return Err(Diagnostic::backend(format!(
            "MIR array literal elements do not match {declared:?}"
        )));
    }
    Ok((declared.clone(), hir::Effects::default()))
}

pub(super) fn verify_array_index(array: Ty, index: Ty) -> Result<(Ty, hir::Effects), Diagnostic> {
    let Ty::Array(_, element) = array.underlying() else {
        return Err(Diagnostic::backend(format!(
            "MIR array index has non-array operand {array:?}"
        )));
    };
    if !is_scalar_element(element) || index != Ty::Int(IntTy::Int) {
        return Err(Diagnostic::backend(format!(
            "invalid MIR array index types: {array:?} indexed by {index:?}"
        )));
    }
    Ok((element.as_ref().clone(), array_effects()))
}

pub(super) fn verify_array_set(
    array: Ty,
    index: Ty,
    value: Ty,
) -> Result<(Ty, hir::Effects), Diagnostic> {
    let Ty::Array(_, element) = array.underlying() else {
        return Err(Diagnostic::backend(format!(
            "MIR array update has non-array operand {array:?}"
        )));
    };
    if !is_scalar_element(element) || index != Ty::Int(IntTy::Int) || value != **element {
        return Err(Diagnostic::backend(format!(
            "invalid MIR array update types: {array:?}[{index:?}] = {value:?}"
        )));
    }
    Ok((array, array_effects()))
}

fn is_scalar_element(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Bool | Ty::Int(IntTy::Int) | Ty::Float(FloatTy::Float64) | Ty::String
    )
}

fn array_effects() -> hir::Effects {
    hir::Effects {
        may_panic: true,
        ..hir::Effects::default()
    }
}
