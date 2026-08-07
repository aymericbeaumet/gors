//! MIR call verification for executable integer pointer operations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{IntTy, Ty};

pub(super) fn is_pointer_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::PointerI64Nil
            | hir::Builtin::PointerI64New
            | hir::Builtin::PointerI64Get
            | hir::Builtin::PointerI64Set
            | hir::Builtin::PointerI64IsNil
            | hir::Builtin::PointerI64Equal
            | hir::Builtin::PointerStructI64Nil
            | hir::Builtin::PointerStructI64New
            | hir::Builtin::PointerStructI64Get
            | hir::Builtin::PointerStructI64Set
            | hir::Builtin::PointerStructI64IsNil
            | hir::Builtin::PointerStructI64Equal
            | hir::Builtin::AggregatePointerNil
            | hir::Builtin::AggregatePointerNew
            | hir::Builtin::AggregatePointerSnapshot
            | hir::Builtin::AggregatePointerIsNil
    )
}

pub(super) fn verify_pointer_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    match builtin {
        hir::Builtin::PointerI64Nil | hir::Builtin::PointerI64New => {
            let ([], [result]) = (arguments, destinations) else {
                return Err(shape_error("pointer creation", arguments, destinations));
            };
            verify_int_pointer_type(result, "pointer creation result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::PointerI64Get => {
            let [pointer] = arguments else {
                return Err(shape_error("pointer dereference", arguments, destinations));
            };
            Ok(vec![
                verify_int_pointer_type(pointer, "pointer dereference")?.clone(),
            ])
        }
        hir::Builtin::PointerI64Set => {
            let [pointer, value] = arguments else {
                return Err(shape_error("pointer assignment", arguments, destinations));
            };
            let element = verify_int_pointer_type(pointer, "pointer assignment")?;
            super::verify_same_type(value, element, "pointer assignment value")?;
            Ok(Vec::new())
        }
        hir::Builtin::PointerI64IsNil => {
            let [pointer] = arguments else {
                return Err(shape_error(
                    "pointer nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_int_pointer_type(pointer, "pointer nil comparison")?;
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::PointerI64Equal => {
            let [left, right] = arguments else {
                return Err(shape_error(
                    "integer pointer equality",
                    arguments,
                    destinations,
                ));
            };
            verify_int_pointer_type(left, "integer pointer equality")?;
            verify_int_pointer_type(right, "integer pointer equality")?;
            super::verify_same_type(left, right, "integer pointer equality")?;
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::PointerStructI64Nil => {
            let ([], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "struct pointer nil value",
                    arguments,
                    destinations,
                ));
            };
            verify_i64_struct_pointer_type(result, "struct pointer nil result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::PointerStructI64New => {
            let ([field_count], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "struct pointer creation",
                    arguments,
                    destinations,
                ));
            };
            verify_int(field_count, "struct pointer field count")?;
            verify_i64_struct_pointer_type(result, "struct pointer creation result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::PointerStructI64Get => {
            let ([pointer, field], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "struct pointer field read",
                    arguments,
                    destinations,
                ));
            };
            let fields = verify_i64_struct_pointer_type(pointer, "struct pointer field read")?;
            verify_int(field, "struct pointer field index")?;
            if !fields.iter().any(|field| field.ty == *result) {
                return Err(shape_error(
                    "struct pointer field read",
                    arguments,
                    destinations,
                ));
            }
            Ok(vec![result.clone()])
        }
        hir::Builtin::PointerStructI64Set => {
            let [pointer, field, value] = arguments else {
                return Err(shape_error(
                    "struct pointer field update",
                    arguments,
                    destinations,
                ));
            };
            let fields = verify_i64_struct_pointer_type(pointer, "struct pointer field update")?;
            verify_int(field, "struct pointer field index")?;
            if !destinations.is_empty() || !fields.iter().any(|field| field.ty == *value) {
                return Err(shape_error(
                    "struct pointer field update",
                    arguments,
                    destinations,
                ));
            }
            Ok(Vec::new())
        }
        hir::Builtin::PointerStructI64IsNil => {
            let [pointer] = arguments else {
                return Err(shape_error(
                    "struct pointer nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_i64_struct_pointer_type(pointer, "struct pointer nil comparison")?;
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::PointerStructI64Equal => {
            let [left, right] = arguments else {
                return Err(shape_error(
                    "struct pointer equality",
                    arguments,
                    destinations,
                ));
            };
            verify_i64_struct_pointer_type(left, "struct pointer equality")?;
            verify_i64_struct_pointer_type(right, "struct pointer equality")?;
            super::verify_same_type(left, right, "struct pointer equality")?;
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::AggregatePointerNil => {
            let ([], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "aggregate pointer nil value",
                    arguments,
                    destinations,
                ));
            };
            verify_aggregate_struct_pointer_type(result, "aggregate pointer nil result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::AggregatePointerNew => {
            let ([identity, snapshot], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "aggregate pointer creation",
                    arguments,
                    destinations,
                ));
            };
            verify_string(identity, "aggregate pointer type identity")?;
            verify_aggregate_snapshot(snapshot, "aggregate pointer snapshot")?;
            verify_aggregate_struct_pointer_type(result, "aggregate pointer creation result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::AggregatePointerSnapshot => {
            let ([pointer, identity], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "aggregate pointer dereference",
                    arguments,
                    destinations,
                ));
            };
            verify_aggregate_struct_pointer_type(pointer, "aggregate pointer dereference")?;
            verify_string(identity, "aggregate pointer type identity")?;
            verify_aggregate_snapshot(result, "aggregate pointer dereference result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::AggregatePointerIsNil => {
            let ([pointer], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "aggregate pointer nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_aggregate_struct_pointer_type(pointer, "aggregate pointer nil comparison")?;
            if result != &Ty::Bool {
                return Err(shape_error(
                    "aggregate pointer nil comparison",
                    arguments,
                    destinations,
                ));
            }
            Ok(vec![Ty::Bool])
        }
        _ => Err(Diagnostic::backend(
            "non-pointer builtin reached pointer MIR verification",
        )),
    }
}

pub(super) fn verify_int_pointer_type<'a>(ty: &'a Ty, context: &str) -> Result<&'a Ty, Diagnostic> {
    let Ty::Pointer(element) = ty.underlying() else {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )));
    };
    if element.underlying() != &Ty::Int(IntTy::Int) {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} element type: {element:?}"
        )));
    }
    Ok(element)
}

fn verify_i64_struct_pointer_type<'a>(
    ty: &'a Ty,
    context: &str,
) -> Result<&'a [crate::compiler::types::StructField], Diagnostic> {
    ty.bootstrap_i64_struct_pointer_fields()
        .ok_or_else(|| Diagnostic::backend(format!("invalid MIR {context} type: {ty:?}")))
}

fn verify_aggregate_struct_pointer_type(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    let Ty::Pointer(element) = ty.underlying() else {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )));
    };
    if element.uses_interface_aggregate_pointer_representation() {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} element type: {element:?}"
        )))
    }
}

fn verify_aggregate_snapshot(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if matches!(
        ty.underlying(),
        Ty::Slice(element) if matches!(element.underlying(), Ty::Interface(_))
    ) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )))
    }
}

fn verify_string(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if ty.underlying() == &Ty::String {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )))
    }
}

fn verify_int(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if ty.underlying() == &Ty::Int(IntTy::Int) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} integer type: {ty:?}"
        )))
    }
}

fn shape_error(context: &str, arguments: &[Ty], destinations: &[Ty]) -> Diagnostic {
    Diagnostic::backend(format!(
        "invalid MIR {context} shape: {arguments:?} -> {destinations:?}"
    ))
}
