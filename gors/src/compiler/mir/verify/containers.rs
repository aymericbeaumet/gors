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

pub(super) fn verify_byte_slice_integer_arguments(
    arguments: &[Ty],
    expected_len: usize,
    context: &str,
) -> Result<(), Diagnostic> {
    let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
    if arguments.len() != expected_len
        || arguments.first() != Some(&byte_slice)
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

pub(super) fn is_aggregate_container_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::AggregateSliceMake
            | hir::Builtin::AggregateSliceLen
            | hir::Builtin::AggregateSliceIndexTagged
            | hir::Builtin::AggregateSliceSetTagged
            | hir::Builtin::AggregateMapMake
            | hir::Builtin::AggregateMapLen
            | hir::Builtin::AggregateMapGetTagged
            | hir::Builtin::AggregateMapContains
            | hir::Builtin::AggregateMapSetTagged
    )
}

pub(super) fn verify_aggregate_container_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    let valid = match builtin {
        hir::Builtin::AggregateSliceMake => {
            matches!(arguments, [Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)])
                && matches!(destinations, [destination] if is_aggregate_slice(destination))
        }
        hir::Builtin::AggregateSliceLen => {
            matches!(arguments, [slice] if is_aggregate_slice(slice))
                && destinations == [Ty::Int(IntTy::Int)]
        }
        hir::Builtin::AggregateSliceIndexTagged => {
            matches!(arguments, [slice, Ty::Int(IntTy::Int)] if is_aggregate_slice(slice))
                && matches!(destinations, [destination] if matches!(destination.underlying(), Ty::Interface(_)))
        }
        hir::Builtin::AggregateSliceSetTagged => {
            matches!(arguments, [slice, Ty::Int(IntTy::Int), tagged]
                if is_aggregate_slice(slice) && matches!(tagged.underlying(), Ty::Interface(_)))
                && destinations.is_empty()
        }
        hir::Builtin::AggregateMapMake => {
            arguments.is_empty()
                && matches!(destinations, [destination] if is_aggregate_map(destination))
        }
        hir::Builtin::AggregateMapLen => {
            matches!(arguments, [map] if is_aggregate_map(map))
                && destinations == [Ty::Int(IntTy::Int)]
        }
        hir::Builtin::AggregateMapGetTagged => {
            matches!(arguments, [map, Ty::String] if is_aggregate_map(map))
                && matches!(destinations, [destination] if matches!(destination.underlying(), Ty::Interface(_)))
        }
        hir::Builtin::AggregateMapContains => {
            matches!(arguments, [map, Ty::String] if is_aggregate_map(map))
                && destinations == [Ty::Bool]
        }
        hir::Builtin::AggregateMapSetTagged => {
            matches!(arguments, [map, Ty::String, tagged]
                if is_aggregate_map(map) && matches!(tagged.underlying(), Ty::Interface(_)))
                && destinations.is_empty()
        }
        _ => false,
    };
    valid.then(|| destinations.to_vec()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid MIR aggregate container call {builtin:?}: {arguments:?} -> {destinations:?}"
        ))
    })
}

fn is_aggregate_slice(ty: &Ty) -> bool {
    matches!(ty.underlying(), Ty::Slice(element) if element.bootstrap_i64_struct_fields().is_some())
}

fn is_aggregate_map(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Map(key, value)
            if key.underlying() == &Ty::String
                && value.bootstrap_i64_struct_fields().is_some()
    )
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
