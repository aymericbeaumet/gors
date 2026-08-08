//! Go semantic type selection for concrete Rust value representations.

use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::RustType;
use crate::compiler::types::{ComplexTy, FloatTy, IntTy, Ty, UintTy};
use gors_runtime_abi::{FloatKind, IntegerKind};

pub(super) const fn float_kind(ty: FloatTy) -> FloatKind {
    match ty {
        FloatTy::Float32 => FloatKind::F32,
        FloatTy::Float64 => FloatKind::F64,
    }
}

pub(super) const fn complex_kind(ty: ComplexTy) -> FloatKind {
    match ty {
        ComplexTy::Complex64 => FloatKind::F32,
        ComplexTy::Complex128 => FloatKind::F64,
    }
}

fn float_kind_for_ty(ty: &Ty) -> Option<FloatKind> {
    match ty.underlying() {
        Ty::Float(ty) => Some(float_kind(*ty)),
        _ => None,
    }
}

pub(super) fn integer_kind(ty: &Ty) -> Option<IntegerKind> {
    Some(match ty.underlying() {
        Ty::Int(IntTy::Int | IntTy::Int64) => IntegerKind::I64,
        Ty::Int(IntTy::Int8) => IntegerKind::I8,
        Ty::Int(IntTy::Int16) => IntegerKind::I16,
        Ty::Int(IntTy::Int32) => IntegerKind::I32,
        Ty::Uint(UintTy::Uint | UintTy::Uint64 | UintTy::Uintptr) => IntegerKind::U64,
        Ty::Uint(UintTy::Uint8) => IntegerKind::U8,
        Ty::Uint(UintTy::Uint16) => IntegerKind::U16,
        Ty::Uint(UintTy::Uint32) => IntegerKind::U32,
        _ => return None,
    })
}

pub(super) fn lower_type(ty: &Ty) -> Result<RustType, Diagnostic> {
    let ty = ty.default_typed();
    match ty.underlying() {
        Ty::Unit => Ok(RustType::Unit),
        Ty::Bool => Ok(RustType::Bool),
        Ty::Int(_) | Ty::Uint(_) => integer_kind(&ty)
            .map(RustType::Integer)
            .ok_or_else(|| Diagnostic::backend(format!("unsupported Go integer type: {ty:?}"))),
        Ty::Float(kind) => Ok(RustType::Float(float_kind(*kind))),
        Ty::Complex(kind) => Ok(RustType::Complex(complex_kind(*kind))),
        Ty::String => Ok(RustType::GoString),
        Ty::Interface(_) => Ok(RustType::GoInterface),
        Ty::Function(_) => Ok(RustType::GoInterface),
        Ty::Slice(element)
            if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
        {
            Ok(RustType::GoSliceI64)
        }
        Ty::Slice(element) if element.snapshot_function_result().is_some() => {
            Ok(RustType::GoSliceI64)
        }
        Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
            Ok(RustType::GoSliceU8)
        }
        Ty::Slice(element) if element.underlying() == &Ty::Bool => Ok(RustType::GoSliceBool),
        Ty::Slice(element) if element.underlying() == &Ty::String => Ok(RustType::GoSliceGoString),
        Ty::Slice(element) if matches!(element.underlying(), Ty::Interface(_)) => {
            Ok(RustType::GoSliceInterface)
        }
        Ty::Slice(element) if element.uses_interface_aggregate_representation() => {
            Ok(RustType::GoSliceInterface)
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::String && value.underlying() == &Ty::Int(IntTy::Int) =>
        {
            Ok(RustType::GoMapStringI64)
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::Int(IntTy::Int) && value.underlying() == &Ty::String =>
        {
            Ok(RustType::GoMapI64GoString)
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::String && value.bootstrap_i64_struct_fields().is_some() =>
        {
            Ok(RustType::GoMapStringInterface)
        }
        Ty::Pointer(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::GoPointerI64)
        }
        Ty::Pointer(element) if element.bootstrap_i64_struct_fields().is_some() => {
            Ok(RustType::GoPointerStructI64)
        }
        Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation() => {
            Ok(RustType::GoInterface)
        }
        Ty::Channel(_, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::GoChannelI64)
        }
        Ty::Channel(_, element) if element.underlying() == &Ty::String => {
            Ok(RustType::GoChannelGoString)
        }
        Ty::Channel(_, element)
            if matches!(
                element.underlying(),
                Ty::Channel(_, nested) if nested.underlying() == &Ty::Int(IntTy::Int)
            ) =>
        {
            Ok(RustType::GoChannelGoChannelI64)
        }
        Ty::Array(length, element) if integer_kind(element).is_some() => {
            Ok(RustType::ArrayInteger {
                length: *length,
                element: integer_kind(element).ok_or_else(|| {
                    Diagnostic::backend("missing integral array element representation")
                })?,
            })
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Bool => {
            Ok(RustType::ArrayBool(*length))
        }
        Ty::Array(length, element) if float_kind_for_ty(element).is_some() => {
            Ok(RustType::ArrayFloat {
                length: *length,
                element: float_kind_for_ty(element).ok_or_else(|| {
                    Diagnostic::backend("missing floating-point array element representation")
                })?,
            })
        }
        Ty::Array(length, element) if element.underlying() == &Ty::String => {
            Ok(RustType::ArrayGoString(*length))
        }
        Ty::Array(length, element) if element.bootstrap_i64_struct_pointer_fields().is_some() => {
            Ok(RustType::ArrayGoPointerStructI64(*length))
        }
        Ty::Array(0, _) => Ok(RustType::ZeroArray),
        Ty::Struct(fields)
            if fields
                .iter()
                .all(|field| field.ty.underlying() == &Ty::Int(IntTy::Int)) =>
        {
            Ok(RustType::StructI64(u64::try_from(fields.len()).map_err(
                |_| Diagnostic::backend("struct representation length does not fit u64"),
            )?))
        }
        Ty::Struct(fields) => Ok(RustType::Struct(
            fields
                .iter()
                .map(|field| lower_type(&field.ty))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        unsupported => Err(Diagnostic::backend(format!(
            "unsupported Go type reached Rust lowering: {unsupported:?}"
        ))),
    }
}
