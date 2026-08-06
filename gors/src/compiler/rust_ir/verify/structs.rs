//! Verification for typed Rust struct representations.

use super::verify_same;
use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::RustType;

pub(super) fn verify_literal(fields: Vec<RustType>, ty: &RustType) -> Result<RustType, Diagnostic> {
    let RustType::Struct(field_types) = ty else {
        return Err(Diagnostic::backend(format!(
            "Rust IR struct literal has a non-struct type: {ty:?}"
        )));
    };
    if fields.len() != field_types.len() {
        return Err(Diagnostic::backend(format!(
            "Rust IR struct literal has {} fields for {} field types",
            fields.len(),
            field_types.len()
        )));
    }
    for (field, field_ty) in fields.into_iter().zip(field_types) {
        verify_same(field, field_ty.clone(), "struct literal field")?;
    }
    Ok(ty.clone())
}

pub(super) fn verify_field(structure: RustType, field: u32) -> Result<RustType, Diagnostic> {
    let RustType::Struct(fields) = structure else {
        return Err(Diagnostic::backend(format!(
            "Rust IR field read has a non-struct operand: {structure:?}"
        )));
    };
    fields
        .get(
            usize::try_from(field).map_err(|_| {
                Diagnostic::backend("Rust IR struct field index does not fit usize")
            })?,
        )
        .cloned()
        .ok_or_else(|| {
            Diagnostic::backend(format!(
                "Rust IR struct field index {field} is out of bounds"
            ))
        })
}

pub(super) fn verify_set(
    structure: RustType,
    field: u32,
    value: RustType,
) -> Result<RustType, Diagnostic> {
    let RustType::Struct(fields) = &structure else {
        return Err(Diagnostic::backend(format!(
            "Rust IR field update has a non-struct operand: {structure:?}"
        )));
    };
    let field_ty =
        fields
            .get(usize::try_from(field).map_err(|_| {
                Diagnostic::backend("Rust IR struct field index does not fit usize")
            })?)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::backend(format!(
                    "Rust IR struct field update {field} is out of bounds"
                ))
            })?;
    verify_same(value, field_ty, "struct field update")?;
    Ok(structure)
}
