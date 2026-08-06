//! MIR type verification for bootstrap integer pointer operations.

use crate::compiler::Diagnostic;
use crate::compiler::types::{IntTy, Ty};

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
