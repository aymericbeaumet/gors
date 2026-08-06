//! MIR type and effect verification for executable integer structs.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::Ty;

pub(super) fn verify_struct_literal(
    fields: Vec<Ty>,
    declared: &Ty,
) -> Result<(Ty, hir::Effects), Diagnostic> {
    let Ty::Struct(definitions) = declared.underlying() else {
        return Err(Diagnostic::backend(
            "MIR struct literal has a non-struct type",
        ));
    };
    if fields.len() != definitions.len() {
        return Err(Diagnostic::backend(
            "MIR struct literal field count does not match its type",
        ));
    }
    for (field, definition) in fields.iter().zip(definitions) {
        super::verify_same_type(field, &definition.ty, "struct literal field")?;
    }
    Ok((declared.clone(), hir::Effects::default()))
}

pub(super) fn verify_struct_field(
    structure: Ty,
    field: u32,
) -> Result<(Ty, hir::Effects), Diagnostic> {
    let definition = field_definition(&structure, field, "read")?;
    Ok((definition.ty, hir::Effects::default()))
}

pub(super) fn verify_struct_set(
    structure: Ty,
    field: u32,
    value: Ty,
) -> Result<(Ty, hir::Effects), Diagnostic> {
    let definition = field_definition(&structure, field, "update")?;
    super::verify_same_type(&value, &definition.ty, "struct field update")?;
    Ok((structure, hir::Effects::default()))
}

fn field_definition(
    structure: &Ty,
    field: u32,
    operation: &str,
) -> Result<crate::compiler::types::StructField, Diagnostic> {
    let Ty::Struct(fields) = structure.underlying() else {
        return Err(Diagnostic::backend(format!(
            "MIR field {operation} has a non-struct operand"
        )));
    };
    fields
        .get(
            usize::try_from(field)
                .map_err(|_| Diagnostic::backend("MIR struct field index does not fit usize"))?,
        )
        .cloned()
        .ok_or_else(|| Diagnostic::backend(format!("MIR struct field {field} is out of bounds")))
}
