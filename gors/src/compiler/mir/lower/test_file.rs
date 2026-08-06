//! Whole-file lowering retained for MIR unit tests.

use super::super::File;
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;

pub(in crate::compiler::mir) fn lower_file(file: &hir::File) -> Result<File, Vec<Diagnostic>> {
    let mut functions = Vec::new();
    let mut diagnostics = Vec::new();
    for function in &file.functions {
        match FunctionLowerer::lower(function) {
            Ok(function) => functions.push(function),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if diagnostics.is_empty() {
        Ok(File {
            package_id: file.package_id,
            package: file.package.clone(),
            functions,
        })
    } else {
        Err(diagnostics)
    }
}
