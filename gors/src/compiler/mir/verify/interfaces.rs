//! MIR call verification for Go interface runtime operations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{IntTy, Ty};

pub(super) fn is_interface_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::InterfaceNil
            | hir::Builtin::InterfaceBoxBool
            | hir::Builtin::InterfaceBoxF64
            | hir::Builtin::InterfaceBoxI64
            | hir::Builtin::InterfaceBoxGoString
            | hir::Builtin::InterfaceBoxGoSliceGoString
            | hir::Builtin::InterfaceBoxStructI64
            | hir::Builtin::InterfaceBoxPointerStructI64
            | hir::Builtin::InterfaceBoxAggregate
            | hir::Builtin::InterfaceBoxComparableAggregate
            | hir::Builtin::InterfaceEqual
            | hir::Builtin::InterfaceIsNil
            | hir::Builtin::InterfaceIsType
            | hir::Builtin::InterfaceIsRuntimeError
            | hir::Builtin::InterfaceUnboxBool
            | hir::Builtin::InterfaceUnboxF64
            | hir::Builtin::InterfaceUnboxI64
            | hir::Builtin::InterfaceUnboxGoString
            | hir::Builtin::InterfaceUnboxGoSliceGoString
            | hir::Builtin::InterfaceStructI64Get
            | hir::Builtin::InterfaceUnboxPointerStructI64
            | hir::Builtin::InterfaceUnboxAggregate
            | hir::Builtin::FunctionNil
            | hir::Builtin::FunctionIsNil
    )
}

pub(super) fn verify_interface_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    match builtin {
        hir::Builtin::InterfaceNil => {
            let ([], [result]) = (arguments, destinations) else {
                return Err(shape_error("interface nil", arguments, destinations));
            };
            verify_interface(result, "interface nil result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::InterfaceBoxBool => {
            verify_box(arguments, destinations, |ty| ty.underlying() == &Ty::Bool)
        }
        hir::Builtin::InterfaceBoxF64 => verify_box(arguments, destinations, |ty| {
            matches!(ty.underlying(), Ty::Float(_))
        }),
        hir::Builtin::InterfaceBoxI64 => {
            verify_box(arguments, destinations, is_i64_interface_scalar)
        }
        hir::Builtin::InterfaceBoxGoString => {
            verify_box(arguments, destinations, |ty| ty.underlying() == &Ty::String)
        }
        hir::Builtin::InterfaceBoxGoSliceGoString => {
            verify_box(arguments, destinations, is_go_string_slice)
        }
        hir::Builtin::InterfaceBoxStructI64 => verify_box(arguments, destinations, |ty| {
            ty.underlying() == &Ty::Slice(Box::new(Ty::Int(IntTy::Int)))
        }),
        hir::Builtin::InterfaceBoxPointerStructI64 => verify_box(arguments, destinations, |ty| {
            ty.bootstrap_i64_struct_pointer_fields().is_some()
        }),
        hir::Builtin::InterfaceBoxAggregate | hir::Builtin::InterfaceBoxComparableAggregate => {
            verify_box(arguments, destinations, is_aggregate_payload)
        }
        hir::Builtin::InterfaceEqual => {
            let ([left, right], [result]) = (arguments, destinations) else {
                return Err(shape_error("interface equality", arguments, destinations));
            };
            verify_interface(left, "interface equality left operand")?;
            verify_interface(right, "interface equality right operand")?;
            if result != &Ty::Bool {
                return Err(shape_error("interface equality", arguments, destinations));
            }
            Ok(vec![Ty::Bool])
        }
        hir::Builtin::InterfaceIsNil => {
            verify_test(arguments, destinations, false, "interface nil test")
        }
        hir::Builtin::InterfaceIsType => {
            verify_test(arguments, destinations, true, "interface dynamic type test")
        }
        hir::Builtin::InterfaceIsRuntimeError => verify_test(
            arguments,
            destinations,
            false,
            "runtime error interface test",
        ),
        hir::Builtin::InterfaceUnboxBool => verify_unbox(
            arguments,
            destinations,
            |ty| ty.underlying() == &Ty::Bool,
            "interface bool extraction",
        ),
        hir::Builtin::InterfaceUnboxF64 => verify_unbox(
            arguments,
            destinations,
            |ty| matches!(ty.underlying(), Ty::Float(_)),
            "interface float extraction",
        ),
        hir::Builtin::InterfaceUnboxI64 => verify_unbox(
            arguments,
            destinations,
            is_i64_interface_scalar,
            "interface integer extraction",
        ),
        hir::Builtin::InterfaceUnboxGoString => verify_unbox(
            arguments,
            destinations,
            |ty| ty.underlying() == &Ty::String,
            "interface string extraction",
        ),
        hir::Builtin::InterfaceUnboxGoSliceGoString => verify_unbox(
            arguments,
            destinations,
            is_go_string_slice,
            "interface string slice extraction",
        ),
        hir::Builtin::InterfaceStructI64Get => {
            let ([interface, identity, field], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "interface struct field extraction",
                    arguments,
                    destinations,
                ));
            };
            verify_interface(interface, "interface struct field extraction")?;
            verify_string(identity, "interface struct type identity")?;
            verify_int(field, "interface struct field index")?;
            verify_int(result, "interface struct field result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::InterfaceUnboxPointerStructI64 => verify_unbox(
            arguments,
            destinations,
            |ty| ty.bootstrap_i64_struct_pointer_fields().is_some(),
            "interface struct pointer extraction",
        ),
        hir::Builtin::InterfaceUnboxAggregate => verify_unbox(
            arguments,
            destinations,
            is_aggregate_payload,
            "interface aggregate extraction",
        ),
        hir::Builtin::FunctionNil => {
            let ([], [result]) = (arguments, destinations) else {
                return Err(shape_error("function nil", arguments, destinations));
            };
            verify_function(result, "function nil result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::FunctionIsNil => {
            let ([function], [result]) = (arguments, destinations) else {
                return Err(shape_error(
                    "function nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_function(function, "function nil comparison")?;
            if result != &Ty::Bool {
                return Err(shape_error(
                    "function nil comparison",
                    arguments,
                    destinations,
                ));
            }
            Ok(vec![Ty::Bool])
        }
        _ => Err(Diagnostic::backend(
            "non-interface builtin reached interface MIR verification",
        )),
    }
}

fn is_i64_interface_scalar(ty: &Ty) -> bool {
    matches!(ty.underlying(), Ty::Int(_) | Ty::Uint(_))
}

fn verify_box(
    arguments: &[Ty],
    destinations: &[Ty],
    payload_matches: impl FnOnce(&Ty) -> bool,
) -> Result<Vec<Ty>, Diagnostic> {
    let ([identity, payload], [result]) = (arguments, destinations) else {
        return Err(shape_error("interface boxing", arguments, destinations));
    };
    verify_string(identity, "interface dynamic type identity")?;
    verify_interface(result, "interface boxing result")?;
    if !payload_matches(payload) {
        return Err(shape_error(
            "interface boxing payload",
            arguments,
            destinations,
        ));
    }
    Ok(vec![result.clone()])
}

fn verify_test(
    arguments: &[Ty],
    destinations: &[Ty],
    has_identity: bool,
    context: &str,
) -> Result<Vec<Ty>, Diagnostic> {
    if destinations != [Ty::Bool] {
        return Err(shape_error(context, arguments, destinations));
    }
    match (has_identity, arguments) {
        (false, [interface]) => verify_interface(interface, context)?,
        (true, [interface, identity]) => {
            verify_interface(interface, context)?;
            verify_string(identity, "interface dynamic type identity")?;
        }
        _ => return Err(shape_error(context, arguments, destinations)),
    }
    Ok(vec![Ty::Bool])
}

fn verify_unbox(
    arguments: &[Ty],
    destinations: &[Ty],
    result_matches: impl FnOnce(&Ty) -> bool,
    context: &str,
) -> Result<Vec<Ty>, Diagnostic> {
    let ([interface, identity], [result]) = (arguments, destinations) else {
        return Err(shape_error(context, arguments, destinations));
    };
    verify_interface(interface, context)?;
    verify_string(identity, "interface dynamic type identity")?;
    if !result_matches(result) {
        return Err(shape_error(context, arguments, destinations));
    }
    Ok(vec![result.clone()])
}

fn verify_interface(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if matches!(ty.underlying(), Ty::Interface(_)) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )))
    }
}

fn verify_function(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if matches!(ty.underlying(), Ty::Function(_)) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )))
    }
}

fn is_aggregate_payload(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Slice(element)
            if element.underlying() != &Ty::String
                && (matches!(element.underlying(), Ty::Interface(_))
                || element.uses_interface_aggregate_representation()
                )
    )
}

fn is_go_string_slice(ty: &Ty) -> bool {
    matches!(ty.underlying(), Ty::Slice(element) if element.underlying() == &Ty::String)
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
            "invalid MIR {context} type: {ty:?}"
        )))
    }
}

fn shape_error(context: &str, arguments: &[Ty], destinations: &[Ty]) -> Diagnostic {
    Diagnostic::backend(format!(
        "invalid MIR {context} shape: arguments {arguments:?}, destinations {destinations:?}"
    ))
}
