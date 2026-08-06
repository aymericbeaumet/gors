//! Go semantic type selection for concrete Rust value representations.

use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::RustType;
use crate::compiler::types::{ComplexTy, FloatTy, IntTy, Ty, UintTy};

pub(super) fn lower_type(ty: &Ty) -> Result<RustType, Diagnostic> {
    let ty = ty.default_typed();
    match ty.underlying() {
        Ty::Unit => Ok(RustType::Unit),
        Ty::Bool => Ok(RustType::Bool),
        Ty::Int(IntTy::Int) => Ok(RustType::I64),
        Ty::Float(FloatTy::Float64) => Ok(RustType::F64),
        Ty::Complex(ComplexTy::Complex128) => Ok(RustType::Complex128),
        Ty::String => Ok(RustType::GoString),
        Ty::Interface(_) => Ok(RustType::GoInterface),
        Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::GoSliceI64)
        }
        Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
            Ok(RustType::GoSliceU8)
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::String && value.underlying() == &Ty::Int(IntTy::Int) =>
        {
            Ok(RustType::GoMapStringI64)
        }
        Ty::Pointer(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::GoPointerI64)
        }
        Ty::Pointer(element) if element.bootstrap_i64_struct_fields().is_some() => {
            Ok(RustType::GoPointerStructI64)
        }
        Ty::Channel(_, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(RustType::GoChannelI64)
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
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
        Ty::Struct(fields)
            if fields
                .iter()
                .all(|field| field.ty.underlying() == &Ty::Int(IntTy::Int)) =>
        {
            Ok(RustType::StructI64(u64::try_from(fields.len()).map_err(
                |_| Diagnostic::backend("struct representation length does not fit u64"),
            )?))
        }
        unsupported => Err(Diagnostic::backend(format!(
            "unsupported Go type reached Rust lowering: {unsupported:?}"
        ))),
    }
}
