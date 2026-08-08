//! MIR verification for Go's diagnostic `print` and `println` builtins.

use crate::compiler::Diagnostic;
use crate::compiler::types::Ty;

pub(super) fn verify_print_arguments(arguments: &[Ty]) -> Result<Vec<Ty>, Diagnostic> {
    for ty in arguments {
        if !ty.is_integer() && !matches!(ty.underlying(), Ty::Bool | Ty::Float(_) | Ty::String) {
            return Err(Diagnostic::backend(format!(
                "print builtin cannot consume MIR operand type {ty:?}"
            )));
        }
    }
    Ok(Vec::new())
}
