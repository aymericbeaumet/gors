//! Source-level semantic facts prepared before Rust AST lowering.

use crate::ast;

use super::{ir, typeinfer::TypeEnv};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FileFacts {
    ir: ir::File,
}

impl FileFacts {
    pub(super) fn lower(file: &ast::File<'_>, env: &TypeEnv) -> Self {
        Self {
            ir: ir::lower_file(file, env),
        }
    }

    pub(super) fn package(&self) -> &str {
        &self.ir.package
    }
}

pub(super) fn validate_file(
    file: &ast::File<'_>,
    type_env: &TypeEnv,
    import_package_names: &BTreeMap<String, String>,
) -> Result<(), super::CompilerError> {
    if let Some(invalid) = ir::invalid_signature_in_file(file) {
        return Err(super::invalid_signature_error(invalid));
    }
    if let Some(invalid) = ir::invalid_receiver_type_in_file(file, type_env) {
        return Err(super::invalid_signature_error(invalid));
    }
    if let Some(invalid) =
        ir::invalid_declaration_in_file_with_import_package_names(file, import_package_names)
    {
        return Err(super::invalid_declaration_error(invalid));
    }
    if let Some(invalid) = ir::invalid_value_declaration_in_file(file, type_env) {
        return Err(super::invalid_declaration_error(invalid));
    }
    if let Some(invalid) = ir::invalid_expression_in_file(file, type_env) {
        return Err(super::invalid_statement_error(invalid));
    }
    if let Some(invalid) = ir::invalid_short_var_redeclaration_in_file(file) {
        return Err(super::invalid_statement_error(invalid));
    }
    validate_unused_locals(file)?;
    Ok(())
}

pub(super) fn validate_unused_imports(
    file: &ast::File<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Result<(), super::CompilerError> {
    if let Some(invalid) =
        ir::invalid_unused_import_in_file_with_import_package_names(file, import_package_names)
    {
        return Err(super::invalid_declaration_error(invalid));
    }
    Ok(())
}

fn validate_unused_locals(file: &ast::File<'_>) -> Result<(), super::CompilerError> {
    if let Some(invalid) = ir::invalid_unused_local_in_file(file) {
        return Err(super::invalid_declaration_error(invalid));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::TypeEnv;
    use crate::parser::parse_file;
    use std::collections::BTreeMap;

    #[test]
    fn file_facts_keep_lowered_ir_available() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    println("ok")
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let facts = FileFacts::lower(&file, &env);

        assert_eq!(facts.package(), "main");
        assert_eq!(facts.ir.items.len(), 1);
    }

    #[test]
    fn validate_file_reports_ir_declaration_errors() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Pair struct {
                    A int
                    A string
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let err = validate_file(&file, &env, &BTreeMap::new()).unwrap_err();

        assert!(
            err.to_string().contains("duplicate field A"),
            "unexpected error: {err}"
        );
    }
}
