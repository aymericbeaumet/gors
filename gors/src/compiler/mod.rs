//! Authoritative Go-to-Rust compiler pipeline.
//!
//! The only production path is:
//!
//! ```text
//! immutable Go source
//!   -> owned structural syntax
//!   -> typed HIR
//!   -> verified explicit-order Go MIR
//!   -> representation-neutral MIR normalization and reverification
//!   -> mandatory Rust representation lowering
//!   -> verified Rust control-flow idiom recognition
//!   -> verified Rust IR
//!   -> terminal syn
//! ```
//!
//! `syn` is a terminal serialization target. It is never inspected to recover
//! semantic facts and is never used to repair semantic lowering.

pub mod db;
mod diagnostic;
mod emit;
pub mod fingerprint;
pub mod hir;
pub mod ids;
pub mod input;
mod lowering;
pub mod mir;
pub mod package_dag;
pub mod provenance;
pub mod rust_ir;
mod scheduler;
mod semantic;
mod session;
pub mod syntax;
pub mod types;

pub use diagnostic::Diagnostic;
pub use scheduler::{CompilerHost, SchedulerTelemetry};
pub use session::CompilerSession;

use std::collections::BTreeMap;
use std::fmt;

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

/// Lower one source file through every authoritative compiler stage.
///
/// This is an internal stage-test helper. Production and fuzz callers enter
/// through [`ProgramInput`](input::ProgramInput) and [`CompilerSession`].
#[cfg(test)]
pub(crate) fn compile_file(filename: &str, source: &str) -> Result<syn::File, Vec<Diagnostic>> {
    let hir = lower_to_hir(filename, source)?;
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
pub(crate) fn lower_to_hir(filename: &str, source: &str) -> Result<hir::File, Vec<Diagnostic>> {
    use std::sync::Arc;

    let mut database = db::CompilerDatabase::new(db::BuildConfig::default());
    let workspace = input::WorkspaceKey::ad_hoc("compiler-stage-tests")
        .map_err(|error| vec![Diagnostic::backend(error.to_string())])?;
    let package = input::PackageKey::command_line();
    let snapshot = input::SourceSnapshot::from_source(filename, source)
        .map(Arc::new)
        .map_err(|error| vec![Diagnostic::backend(error.to_string())])?;
    let file = database
        .set_source(&workspace, &package, filename, snapshot)
        .map_err(query_diagnostics)?
        .file();
    let analysis = database.analyze_file(file).map_err(query_diagnostics)?;
    let mut functions = analysis
        .functions()
        .iter()
        .map(|function| {
            database
                .typed_hir(file, function.id())
                .map(|function| function.function().clone())
                .map_err(query_diagnostics)
        })
        .collect::<Result<Vec<_>, _>>()?;
    functions.sort_by_key(|function| function.id);
    Ok(hir::File {
        package: analysis.package().to_string(),
        constants: Vec::new(),
        functions,
    })
}

#[cfg(test)]
fn query_diagnostics(error: db::QueryError) -> Vec<Diagnostic> {
    match error {
        db::QueryError::StageFailure(failure) => failure.diagnostics().to_vec(),
        error => vec![Diagnostic::backend(error.to_string())],
    }
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

    fn terminal(diagnostic: Diagnostic) -> Self {
        let (code, message) = match diagnostic.location {
            diagnostic::DiagnosticLocation::Synthetic => (diagnostic.code, diagnostic.message),
            location => (
                "GORS2003",
                format!(
                    "terminal emission returned unresolved source location {location:?}: {}",
                    diagnostic.message
                ),
            ),
        };
        Self {
            diagnostics: vec![CompilerDiagnostic {
                code,
                message,
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

/// Complete output of one compiler invocation.
///
/// The entry unit is separate from dependency modules, so consumers never
/// infer module roles from flags or sentinel map keys.
#[derive(Clone)]
pub struct CompiledProgram {
    pub entry: syn::File,
    pub modules: BTreeMap<String, syn::File>,
    /// Exact target-neutral runtime contract and operations selected by the
    /// verified Rust IR package.
    ///
    /// This dependency is unconditional, including when its operation set is
    /// empty, because runtime-backed value representations are not yet
    /// independently sliceable.
    pub runtime: gors_runtime_abi::RuntimeDependency,
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
