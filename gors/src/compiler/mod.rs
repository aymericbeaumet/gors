//! Authoritative Go-to-Rust compiler pipeline.
//!
//! The only production path is:
//!
//! ```text
//! parsed Go AST
//!   -> typed HIR
//!   -> verified explicit-order Go MIR
//!   -> representation-neutral MIR normalization and reverification
//!   -> mandatory Rust representation lowering
//!   -> verified Rust IR
//!   -> terminal syn
//! ```
//!
//! `syn` is a terminal serialization target. It is never inspected to recover
//! semantic facts and is never repaired by a post-lowering compatibility pass.

mod diagnostic;
mod emit;
pub mod hir;
pub mod ids;
mod lowering;
pub mod mir;
pub mod rust_ir;
mod semantic;
pub mod types;

pub use diagnostic::Diagnostic;
pub use hir::File as HirFile;
pub use rust_ir::File as RustIrFile;

use std::collections::BTreeMap;
use std::fmt;

use crate::ast;

/// MIR that has passed whole-file structural, type, call-ABI, and dataflow
/// verification.
///
/// The raw MIR types remain public for diagnostics and tooling, but only this
/// opaque product can enter mandatory normalization and Rust representation
/// lowering. Consumers get read-only access so mutating MIR always requires a
/// fresh verification boundary.
#[derive(Clone, Debug)]
pub struct VerifiedMir(mir::File);

impl VerifiedMir {
    /// Inspect the verified stage product without making it mutable.
    #[must_use]
    pub fn as_file(&self) -> &mir::File {
        &self.0
    }

    fn verify(file: mir::File) -> Result<Self, Vec<Diagnostic>> {
        mir::verify(&file).map_err(|diagnostic| vec![diagnostic])?;
        Ok(Self(file))
    }

    fn into_inner(self) -> mir::File {
        self.0
    }

    fn from_verified(file: mir::File) -> Self {
        Self(file)
    }
}

/// Rust-oriented IR whose representation and local-use decisions have passed
/// verification.
///
/// This mandatory stage separates Go semantics from Rust ownership and ABI
/// choices. Terminal syntax emission accepts no earlier stage product.
#[derive(Clone, Debug)]
pub struct VerifiedRustIr(rust_ir::File);

impl VerifiedRustIr {
    /// Inspect the verified Rust representation IR without making it mutable.
    #[must_use]
    pub fn as_file(&self) -> &rust_ir::File {
        &self.0
    }

    fn verify(file: rust_ir::File) -> Result<Self, Vec<Diagnostic>> {
        rust_ir::verify(&file).map_err(|diagnostic| vec![diagnostic])?;
        Ok(Self(file))
    }
}

/// Lower a parsed Go file through every authoritative compiler stage.
pub fn compile_file(file: &ast::File<'_>) -> Result<syn::File, Vec<Diagnostic>> {
    let hir = lower_to_hir(file)?;
    let mir = lower_to_mir(&hir)?;
    let rust_ir = lower_to_rust_ir(mir)?;
    emit_rust_ir(&rust_ir)
}

/// Produce typed, name-resolved HIR without committing to a Rust
/// representation.
pub fn lower_to_hir(file: &ast::File<'_>) -> Result<HirFile, Vec<Diagnostic>> {
    semantic::lower_file(file)
}

/// Produce evaluation-order-explicit Go MIR.
pub fn lower_to_mir(file: &HirFile) -> Result<VerifiedMir, Vec<Diagnostic>> {
    VerifiedMir::verify(mir::lower_file(file)?)
}

/// Select Rust representations and local-use behavior from verified Go MIR.
///
/// MIR normalization is mandatory inside this function; there is no bypass or
/// alternate emitter entry point.
pub fn lower_to_rust_ir(file: VerifiedMir) -> Result<VerifiedRustIr, Vec<Diagnostic>> {
    VerifiedRustIr::verify(lowering::lower(file)?)
}

/// Emit verified Rust representation IR into terminal Rust syntax.
pub fn emit_rust_ir(file: &VerifiedRustIr) -> Result<syn::File, Vec<Diagnostic>> {
    emit::emit_file(file.as_file()).map_err(|diagnostic| vec![diagnostic])
}

/// A diagnostic emitted by the semantic pipeline.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompilerDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for CompilerDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.file.is_empty() {
            write!(f, "{}: {}", self.code, self.message)
        } else {
            write!(
                f,
                "{}:{}:{}: {}: {}",
                self.file, self.line, self.column, self.code, self.message
            )
        }
    }
}

/// Compilation failed before Rust emission.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompilerError {
    diagnostics: Vec<CompilerDiagnostic>,
}

impl CompilerError {
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![CompilerDiagnostic {
                code: "GORS2001",
                message: message.into(),
                file: String::new(),
                line: 0,
                column: 0,
            }],
        }
    }

    /// Structured diagnostics in deterministic source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[CompilerDiagnostic] {
        &self.diagnostics
    }
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index != 0 {
                f.write_str("\n")?;
            }
            write!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CompilerError {}

impl From<Vec<Diagnostic>> for CompilerError {
    fn from(mut diagnostics: Vec<Diagnostic>) -> Self {
        diagnostics.sort_by(|left, right| {
            left.span
                .file
                .cmp(&right.span.file)
                .then_with(|| left.span.start.cmp(&right.span.start))
                .then_with(|| left.code.cmp(right.code))
                .then_with(|| left.message.cmp(&right.message))
        });
        Self {
            diagnostics: diagnostics
                .into_iter()
                .map(|diagnostic| CompilerDiagnostic {
                    code: diagnostic.code,
                    message: diagnostic.message,
                    file: diagnostic.span.file,
                    line: diagnostic.span.line,
                    column: diagnostic.span.column,
                })
                .collect(),
        }
    }
}

/// Complete output of one compiler invocation.
///
/// The entry unit is separate from dependency modules, so consumers never
/// infer module roles from flags or sentinel map keys.
#[derive(Clone)]
pub struct CompiledProgram {
    pub entry: syn::File,
    pub modules: BTreeMap<String, syn::File>,
}

/// Immutable inputs required to map one generated Rust artifact back to Go.
///
/// Source-map state is an explicit compilation product. It is safe to retain,
/// build on another thread, or use concurrently with another compilation.
pub struct SourceMapPlan {
    tracker: crate::sourcemap::SourceMapTracker,
}

impl SourceMapPlan {
    /// Build Source Map v3 data for the final formatted Rust entry unit.
    #[must_use]
    pub fn build(&self, rust_source: &str) -> sourcemap::SourceMap {
        self.tracker.build_source_map(rust_source)
    }
}

/// Compile one parsed Go file to terminal Rust syntax.
///
/// This does not package the runtime. The API exists for compiler-stage tests
/// and tooling; use [`compile_program`] for a self-contained generated program.
pub fn compile_file_to_rust_syntax(file: ast::File<'_>) -> Result<syn::File, CompilerError> {
    compile_file(&file).map_err(CompilerError::from)
}

/// Compile a parsed program into deterministic, self-contained Rust units.
///
/// The bootstrap backend deliberately accepts exactly one import-free `main`
/// source file. The boundary rejects wider parser products before lowering so
/// a merged package can never masquerade as a correctly modeled single file.
pub fn compile_program(
    program: crate::parser::ParsedProgram,
) -> Result<CompiledProgram, CompilerError> {
    compile_program_impl(program, false).map(|(compiled, _)| compiled)
}

/// Compile a program and return its explicit source-map plan.
pub fn compile_program_with_source_map(
    program: crate::parser::ParsedProgram,
) -> Result<(CompiledProgram, SourceMapPlan), CompilerError> {
    compile_program_impl(program, true).and_then(|(compiled, plan)| {
        plan.map(|plan| (compiled, plan))
            .ok_or_else(|| CompilerError::unsupported("source-map plan was not constructed"))
    })
}

fn compile_program_impl(
    program: crate::parser::ParsedProgram,
    with_source_map: bool,
) -> Result<(CompiledProgram, Option<SourceMapPlan>), CompilerError> {
    validate_bootstrap_program(&program)?;

    let source_map = with_source_map.then(|| source_map_plan(&program));

    let file = compile_file_to_rust_syntax(program.main_package.ast);
    let entry = file?;

    Ok((
        CompiledProgram {
            entry,
            modules: BTreeMap::new(),
        },
        source_map,
    ))
}

fn source_map_plan(program: &crate::parser::ParsedProgram) -> SourceMapPlan {
    let mut tracker = crate::sourcemap::SourceMapTracker::new();
    tracker.start_many(
        program
            .main_package
            .files
            .iter()
            .map(|(name, source)| (name.clone(), Some(source.clone())))
            .collect(),
        "main.rs",
    );
    record_source_landmarks(&mut tracker, &program.main_package.ast);
    tracker.pause();
    SourceMapPlan { tracker }
}

fn validate_bootstrap_program(program: &crate::parser::ParsedProgram) -> Result<(), CompilerError> {
    if program.main_package.files.len() != 1 {
        return Err(program_error(
            program,
            "the bootstrap backend requires exactly one Go source file",
        ));
    }
    if !program.imports.is_empty() || !program.stdlib_imports.is_empty() {
        return Err(program_error(
            program,
            "imports are not implemented by the HIR/MIR backend",
        ));
    }
    if program.main_package.name != "main" {
        return Err(program_error(
            program,
            "executable compilation requires package main",
        ));
    }
    let main_functions = program
        .main_package
        .ast
        .decls
        .iter()
        .filter_map(|decl| match decl {
            ast::Decl::FuncDecl(function) if function.name.name == "main" => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [main] = main_functions.as_slice() else {
        return Err(program_error(
            program,
            "package main must declare exactly one main function",
        ));
    };
    if !main.type_.params.list.is_empty()
        || main
            .type_
            .results
            .as_ref()
            .is_some_and(|results| !results.list.is_empty())
    {
        return Err(program_error(
            program,
            "func main must have no parameters or results",
        ));
    }
    Ok(())
}

fn program_error(
    program: &crate::parser::ParsedProgram,
    message: impl Into<String>,
) -> CompilerError {
    let position = &program.main_package.ast.package;
    let file = if position.file.is_empty() {
        program
            .main_package
            .files
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_default()
    } else if position.directory.is_empty() || position.file.starts_with('/') {
        position.file.to_string()
    } else {
        format!("{}/{}", position.directory, position.file)
    };
    CompilerError {
        diagnostics: vec![CompilerDiagnostic {
            code: "GORS2001",
            message: message.into(),
            file,
            line: position.line,
            column: position.column,
        }],
    }
}

fn record_source_landmarks(tracker: &mut crate::sourcemap::SourceMapTracker, file: &ast::File<'_>) {
    record_mapping(tracker, &file.package, Some("package"));
    record_mapping(tracker, &file.name.name_pos, Some(file.name.name));
    for declaration in &file.decls {
        if let ast::Decl::FuncDecl(function) = declaration {
            if let Some(position) = &function.type_.func {
                record_mapping(tracker, position, Some("func"));
            }
            record_mapping(tracker, &function.name.name_pos, Some(function.name.name));
        }
    }
}

fn record_mapping(
    tracker: &mut crate::sourcemap::SourceMapTracker,
    position: &crate::token::Position<'_>,
    name: Option<&str>,
) {
    let source = if position.file.is_empty() {
        None
    } else if position.directory.is_empty() || position.file.starts_with('/') {
        Some(position.file.to_string())
    } else {
        Some(format!("{}/{}", position.directory, position.file))
    };
    tracker.record_for_source(source, position.line as u32, position.column as u32, name);
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
