//! Rust representation selection for Go struct operations.

use super::{lower_operand, mir_operand_type};
use crate::compiler::Diagnostic;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use crate::compiler::types::Ty;

pub(super) fn lower_struct_literal(
    fields: Vec<mir::Operand>,
    ty: Ty,
    locals: &[out::LocalDecl],
) -> Result<out::RvalueKind, Diagnostic> {
    let rust_ty = super::lower_type(&ty)?;
    let expected_length = match &rust_ty {
        out::RustType::StructI64(length) => usize::try_from(*length)
            .map_err(|_| Diagnostic::backend("struct representation length does not fit usize"))?,
        out::RustType::Struct(fields) => fields.len(),
        _ => {
            return Err(Diagnostic::backend(
                "non-struct reached struct representation lowering",
            ));
        }
    };
    if fields.len() != expected_length {
        return Err(Diagnostic::backend(
            "struct field count changed during representation lowering",
        ));
    }
    let fields = fields
        .into_iter()
        .map(|field| lower_operand(field, locals))
        .collect::<Result<Vec<_>, _>>()?;
    match rust_ty {
        out::RustType::StructI64(_) => Ok(out::RvalueKind::StructLiteralI64(fields)),
        ty @ out::RustType::Struct(_) => Ok(out::RvalueKind::StructLiteral { fields, ty }),
        _ => Err(Diagnostic::backend(
            "validated struct representation changed during lowering",
        )),
    }
}

pub(super) fn lower_struct_field(
    structure: mir::Operand,
    field: u32,
    locals: &[out::LocalDecl],
) -> Result<out::RvalueKind, Diagnostic> {
    let structure_ty = mir_operand_type(&structure, locals)?;
    let structure = lower_operand(structure, locals)?;
    match structure_ty {
        out::RustType::StructI64(length) if u64::from(field) < length => {
            Ok(out::RvalueKind::StructFieldI64 { structure, field })
        }
        out::RustType::Struct(fields)
            if usize::try_from(field).is_ok_and(|field| field < fields.len()) =>
        {
            Ok(out::RvalueKind::StructField { structure, field })
        }
        out::RustType::StructI64(_) | out::RustType::Struct(_) => Err(Diagnostic::backend(
            "struct field index is outside its representation",
        )),
        _ => Err(Diagnostic::backend(
            "non-struct reached field representation lowering",
        )),
    }
}

pub(super) fn lower_struct_set(
    structure: mir::Operand,
    field: u32,
    value: mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::RvalueKind, Diagnostic> {
    let structure_ty = mir_operand_type(&structure, locals)?;
    let value_ty = mir_operand_type(&value, locals)?;
    let structure = lower_operand(structure, locals)?;
    let value = lower_operand(value, locals)?;
    match structure_ty {
        out::RustType::StructI64(length)
            if u64::from(field) < length
                && value_ty == out::RustType::Integer(gors_runtime_abi::IntegerKind::I64) =>
        {
            Ok(out::RvalueKind::StructSetI64 {
                structure,
                field,
                value,
            })
        }
        out::RustType::Struct(fields)
            if usize::try_from(field)
                .ok()
                .and_then(|field| fields.get(field))
                == Some(&value_ty) =>
        {
            Ok(out::RvalueKind::StructSet {
                structure,
                field,
                value,
            })
        }
        out::RustType::StructI64(_) | out::RustType::Struct(_) => Err(Diagnostic::backend(
            "invalid struct field update reached representation lowering",
        )),
        _ => Err(Diagnostic::backend(
            "non-struct reached field update representation lowering",
        )),
    }
}
