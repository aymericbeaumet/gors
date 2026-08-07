//! MIR verification helpers for slice and map runtime operations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{IntTy, Ty, UintTy};

pub(super) fn is_representation_slice_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::SliceI64Nil
            | hir::Builtin::SliceI64IsNil
            | hir::Builtin::SliceU8Nil
            | hir::Builtin::SliceU8IsNil
            | hir::Builtin::SliceBoolNil
            | hir::Builtin::SliceBoolIsNil
            | hir::Builtin::AggregateSliceNil
            | hir::Builtin::AggregateSliceIsNil
            | hir::Builtin::SnapshotFunctionSliceAppend
            | hir::Builtin::SnapshotFunctionSliceCall
    )
}

pub(super) fn is_go_string_slice_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::SliceGoStringIndex
            | hir::Builtin::SliceGoStringRange
            | hir::Builtin::SliceGoStringSet
            | hir::Builtin::SliceGoStringMake
            | hir::Builtin::SliceGoStringNil
            | hir::Builtin::SliceGoStringIsNil
            | hir::Builtin::SliceGoStringLen
            | hir::Builtin::SliceGoStringCap
            | hir::Builtin::SliceGoStringAppend
            | hir::Builtin::SliceGoStringCopy
            | hir::Builtin::SliceGoStringClear
    )
}

pub(super) fn verify_go_string_slice_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    let string_slice = |ty: &Ty| matches!(ty.underlying(), Ty::Slice(element) if element.underlying() == &Ty::String);
    let element = |ty: &Ty| match ty.underlying() {
        Ty::Slice(element) if element.underlying() == &Ty::String => Some(element.as_ref().clone()),
        _ => None,
    };
    let valid = match builtin {
        hir::Builtin::SliceGoStringNil => {
            arguments.is_empty() && matches!(destinations, [slice] if string_slice(slice))
        }
        hir::Builtin::SliceGoStringMake => {
            arguments == [Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)]
                && matches!(destinations, [slice] if string_slice(slice))
        }
        hir::Builtin::SliceGoStringLen | hir::Builtin::SliceGoStringCap => {
            matches!(arguments, [slice] if string_slice(slice))
                && destinations == [Ty::Int(IntTy::Int)]
        }
        hir::Builtin::SliceGoStringIndex => {
            matches!((arguments, destinations), ([slice, Ty::Int(IntTy::Int)], [value])
                if element(slice).as_ref() == Some(value))
        }
        hir::Builtin::SliceGoStringRange => {
            matches!(
                (arguments, destinations),
                (
                    [slice, Ty::Int(IntTy::Int), Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)],
                    [result]
                ) if string_slice(slice) && result == slice
            )
        }
        hir::Builtin::SliceGoStringSet => {
            matches!(arguments, [slice, Ty::Int(IntTy::Int), value]
                if element(slice).as_ref() == Some(value))
                && destinations.is_empty()
        }
        hir::Builtin::SliceGoStringAppend => {
            matches!(
                (arguments, destinations),
                ([slice, value], [result])
                    if element(slice).as_ref() == Some(value) && result == slice
            )
        }
        hir::Builtin::SliceGoStringCopy => {
            matches!(arguments, [destination, source]
                if string_slice(destination)
                    && string_slice(source)
                    && element(destination) == element(source))
                && destinations == [Ty::Int(IntTy::Int)]
        }
        hir::Builtin::SliceGoStringClear => {
            matches!(arguments, [slice] if string_slice(slice)) && destinations.is_empty()
        }
        hir::Builtin::SliceGoStringIsNil => {
            matches!(arguments, [slice] if string_slice(slice)) && destinations == [Ty::Bool]
        }
        _ => false,
    };
    valid.then(|| destinations.to_vec()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid MIR string slice call {builtin:?}: {arguments:?} -> {destinations:?}"
        ))
    })
}

pub(super) fn verify_representation_slice_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    match builtin {
        hir::Builtin::SliceI64Nil => {
            let [destination] = destinations else {
                return Err(Diagnostic::backend(
                    "integer slice nil operation requires one destination",
                ));
            };
            if !arguments.is_empty()
                || !matches!(
                    destination.underlying(),
                    Ty::Slice(element)
                        if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32))
                            || element.snapshot_function_result().is_some()
                )
            {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR integer slice nil operation: {arguments:?} -> {destinations:?}"
                )));
            }
            Ok(vec![destination.clone()])
        }
        hir::Builtin::SliceI64IsNil => {
            if !matches!(
                arguments,
                [Ty::Slice(element)]
                    if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32))
                        || element.snapshot_function_result().is_some()
            ) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR integer slice nil test: {arguments:?}"
                )));
            }
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::SliceU8Nil => verify_fixed_slice_nil(
            arguments,
            destinations,
            Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8))),
            "byte",
        ),
        hir::Builtin::SliceU8IsNil => verify_fixed_slice_nil_test(
            arguments,
            Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8))),
            "byte",
        ),
        hir::Builtin::SliceBoolNil => verify_fixed_slice_nil(
            arguments,
            destinations,
            Ty::Slice(Box::new(Ty::Bool)),
            "bool",
        ),
        hir::Builtin::SliceBoolIsNil => {
            verify_fixed_slice_nil_test(arguments, Ty::Slice(Box::new(Ty::Bool)), "bool")
        }
        hir::Builtin::AggregateSliceNil => {
            let [destination] = destinations else {
                return Err(Diagnostic::backend(
                    "aggregate slice nil operation requires one destination",
                ));
            };
            if !arguments.is_empty() || !matches!(destination.underlying(), Ty::Slice(_)) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR aggregate slice nil operation: {arguments:?} -> {destinations:?}"
                )));
            }
            Ok(vec![destination.clone()])
        }
        hir::Builtin::AggregateSliceIsNil => {
            if !matches!(arguments, [Ty::Slice(_)]) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR aggregate slice nil test: {arguments:?}"
                )));
            }
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::SnapshotFunctionSliceAppend => {
            let [slice, capture] = arguments else {
                return Err(Diagnostic::backend(
                    "snapshot function append has invalid arity",
                ));
            };
            let Some(function) = slice.snapshot_function_slice_element() else {
                return Err(Diagnostic::backend(
                    "snapshot function append has a non-function slice",
                ));
            };
            if function.snapshot_function_result() != Some(capture) {
                return Err(Diagnostic::backend(
                    "snapshot function append capture type changed",
                ));
            }
            Ok(vec![slice.clone()])
        }
        hir::Builtin::SnapshotFunctionSliceCall => {
            let [slice, index] = arguments else {
                return Err(Diagnostic::backend(
                    "snapshot function call has invalid arity",
                ));
            };
            if index != &Ty::Int(IntTy::Int) {
                return Err(Diagnostic::backend(
                    "snapshot function call has a non-integer index",
                ));
            }
            let result = slice
                .snapshot_function_slice_element()
                .and_then(Ty::snapshot_function_result)
                .cloned()
                .ok_or_else(|| {
                    Diagnostic::backend("snapshot function call has a non-function slice")
                })?;
            Ok(vec![result])
        }
        _ => Err(Diagnostic::backend(
            "slice representation verifier received another operation",
        )),
    }
}

fn verify_fixed_slice_nil(
    arguments: &[Ty],
    destinations: &[Ty],
    slice: Ty,
    name: &str,
) -> Result<Vec<Ty>, Diagnostic> {
    if !arguments.is_empty() || destinations != [slice.clone()] {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {name} slice nil operation: {arguments:?} -> {destinations:?}"
        )));
    }
    Ok(vec![slice])
}

fn verify_fixed_slice_nil_test(
    arguments: &[Ty],
    slice: Ty,
    name: &str,
) -> Result<Vec<Ty>, Diagnostic> {
    if arguments != [slice] {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {name} slice nil test: {arguments:?}"
        )));
    }
    Ok(vec![Ty::Bool])
}

pub(super) fn verify_slice_call_arguments(
    arguments: &[Ty],
    expected_len: usize,
    context: &str,
) -> Result<Ty, Diagnostic> {
    let element = arguments.first().and_then(|ty| match ty.underlying() {
        Ty::Slice(element)
            if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
        {
            Some(element.as_ref().clone())
        }
        _ => None,
    });
    if arguments.len() != expected_len
        || element.is_none()
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
    element.ok_or_else(|| Diagnostic::backend(format!("invalid MIR {context} slice type")))
}

pub(super) fn verify_slice_value_arguments(
    arguments: &[Ty],
    indexed: bool,
    context: &str,
) -> Result<Ty, Diagnostic> {
    let expected_len = if indexed { 3 } else { 2 };
    let element = arguments.first().and_then(|ty| match ty.underlying() {
        Ty::Slice(element)
            if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
        {
            Some(element.as_ref().clone())
        }
        _ => None,
    });
    let index_valid = !indexed || arguments.get(1) == Some(&Ty::Int(IntTy::Int));
    if arguments.len() != expected_len || !index_valid || arguments.last() != element.as_ref() {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} argument types: {arguments:?}"
        )));
    }
    element
        .map(|element| Ty::Slice(Box::new(element)))
        .ok_or_else(|| Diagnostic::backend(format!("invalid MIR {context} slice type")))
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
    matches!(
        ty.underlying(),
        Ty::Slice(element)
            if element.underlying() != &Ty::String
                && (matches!(element.underlying(), Ty::Interface(_))
                || element.uses_interface_aggregate_representation()
                )
    )
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
