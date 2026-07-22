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

pub mod db;
mod diagnostic;
mod emit;
pub mod fingerprint;
pub mod hir;
pub mod ids;
pub mod input;
mod lowering;
pub mod mir;
pub mod rust_ir;
mod scheduler;
mod semantic;
mod session;
pub mod source;
pub mod types;

pub use diagnostic::Diagnostic;
pub use scheduler::{CompilerHost, SchedulerTelemetry};
pub use session::CompilerSession;

use std::collections::BTreeMap;
use std::fmt;

#[cfg(test)]
use crate::ast;

/// MIR that has passed whole-file structural, type, call-ABI, and dataflow
/// verification.
///
/// The raw MIR types remain public for diagnostics and tooling, but only this
/// opaque product can enter mandatory normalization and Rust representation
/// lowering. Consumers get read-only access so mutating MIR always requires a
/// fresh verification boundary.
#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct VerifiedMir(mir::File);

#[cfg(test)]
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
#[cfg(test)]
pub(crate) struct VerifiedRustIr(rust_ir::File);

#[cfg(test)]
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
///
/// This is an internal stage-test helper. Production and fuzz callers enter
/// through [`ProgramInput`](input::ProgramInput) and [`CompilerSession`].
#[cfg(test)]
pub(crate) fn compile_file(file: &ast::File<'_>) -> Result<syn::File, Vec<Diagnostic>> {
    let hir = lower_to_hir(file)?;
    compile_hir(hir)
}

#[cfg(test)]
fn compile_hir(hir: hir::File) -> Result<syn::File, Vec<Diagnostic>> {
    let mir = lower_to_mir(&hir)?;
    let rust_ir = lower_to_rust_ir(mir)?;
    emit_rust_ir(&rust_ir)
}

/// Produce typed, name-resolved HIR without committing to a Rust
/// representation.
#[cfg(test)]
pub(crate) fn lower_to_hir(file: &ast::File<'_>) -> Result<hir::File, Vec<Diagnostic>> {
    semantic::lower_file(file)
}

/// Produce evaluation-order-explicit Go MIR.
#[cfg(test)]
pub(crate) fn lower_to_mir(file: &hir::File) -> Result<VerifiedMir, Vec<Diagnostic>> {
    VerifiedMir::verify(mir::lower_file(file)?)
}

/// Select Rust representations and local-use behavior from verified Go MIR.
///
/// MIR normalization is mandatory inside this function; there is no bypass or
/// alternate emitter entry point.
#[cfg(test)]
pub(crate) fn lower_to_rust_ir(file: VerifiedMir) -> Result<VerifiedRustIr, Vec<Diagnostic>> {
    VerifiedRustIr::verify(lowering::lower(file)?)
}

/// Emit verified Rust representation IR into terminal Rust syntax.
#[cfg(test)]
pub(crate) fn emit_rust_ir(file: &VerifiedRustIr) -> Result<syn::File, Vec<Diagnostic>> {
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

    fn backend(message: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![CompilerDiagnostic {
                code: "GORS2003",
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
    entry_source_name: std::sync::Arc<str>,
    entry_comments: std::sync::Arc<db::FileComments>,
}

impl SourceMapPlan {
    /// Build Source Map v3 data for the final formatted Rust entry unit.
    #[must_use]
    pub fn build(&self, rust_source: &str) -> sourcemap::SourceMap {
        self.tracker.build_source_map(rust_source)
    }

    /// Presentation name of the entry source represented by
    /// [`Self::entry_comments`].
    #[must_use]
    pub fn entry_source_name(&self) -> &str {
        &self.entry_source_name
    }

    /// Query-owned comments for the entry source revision compiled into this
    /// plan.
    ///
    /// The comments remain tied to this immutable plan even after its
    /// originating [`CompilerSession`] installs a later source revision.
    #[must_use]
    pub fn entry_comments(&self) -> &db::FileComments {
        self.entry_comments.as_ref()
    }
}

/// Compile raw program inputs into deterministic, self-contained Rust units.
///
/// The bootstrap backend deliberately accepts exactly one import-free `main`
/// source file. Wider package manifests are rejected before lowering until
/// cross-package semantic indexing is implemented.
pub fn compile_program(program: input::ProgramInput) -> Result<CompiledProgram, CompilerError> {
    compile_program_impl(program, false).map(|(compiled, _)| compiled)
}

/// Compile a program and return its explicit source-map plan.
pub fn compile_program_with_source_map(
    program: input::ProgramInput,
) -> Result<(CompiledProgram, SourceMapPlan), CompilerError> {
    compile_program_impl(program, true).and_then(|(compiled, plan)| {
        plan.map(|plan| (compiled, plan))
            .ok_or_else(|| CompilerError::unsupported("source-map plan was not constructed"))
    })
}

fn compile_program_impl(
    program: input::ProgramInput,
    with_source_map: bool,
) -> Result<(CompiledProgram, Option<SourceMapPlan>), CompilerError> {
    // The free facade is intentionally one-shot. Avoid constructing native
    // workers that cannot be reused; retained callers and the CLI should own a
    // CompilerHost with an explicit job budget.
    let mut session = CompilerHost::inline().session(db::BuildConfig::default())?;
    if with_source_map {
        let (compiled, plan) = session.compile_program_with_source_map(program)?;
        Ok((compiled, Some(plan)))
    } else {
        session
            .compile_program(program)
            .map(|compiled| (compiled, None))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
