//! Go semantic type selection for concrete Rust value representations.

use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::RustType;
use crate::compiler::types::{ComplexTy, FloatTy, IntTy, Ty, UintTy};

pub(super) fn lower_type(ty: &Ty) -> Result<RustType, Diagnostic> {
    let ty = ty.default_typed();
    match ty.underlying() {
        Ty::Unit => Ok(RustType::Unit),
        Ty::Bool => Ok(RustType::Bool),
        Ty::Int(IntTy::Int | IntTy::Int8 | IntTy::Int32)
        | Ty::Uint(UintTy::Uint | UintTy::Uint8 | UintTy::Uintptr) => Ok(RustType::I64),
        Ty::Float(FloatTy::Float32 | FloatTy::Float64) => Ok(RustType::F64),
        Ty::Complex(ComplexTy::Complex128) => Ok(RustType::Complex128),
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
        Ty::Array(0, _) => Ok(RustType::ArrayI64(0)),
        Ty::Array(length, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::ArrayI64(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
            Ok(RustType::ArrayI64(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Bool => {
            Ok(RustType::ArrayBool(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Float(FloatTy::Float64) => {
            Ok(RustType::ArrayF64(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::String => {
            Ok(RustType::ArrayGoString(*length))
        }
        Ty::Array(length, element) if element.bootstrap_i64_struct_pointer_fields().is_some() => {
            Ok(RustType::ArrayGoPointerStructI64(*length))
        }
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
