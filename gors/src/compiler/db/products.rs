//! Immutable products of the executable compiler query spine.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::compiler::fingerprint::{self, Fingerprint, fingerprint_parts};
use crate::compiler::ids::{DefId, FileId};
use crate::compiler::{Diagnostic, hir, mir, rust_ir};

/// Authoritative stage at which a tracked compilation failed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompilerStage {
    Semantic,
    GoMir,
    GoMirNormalization,
    RustRepresentation,
}

/// Deterministic structured failure retained as an ordinary query value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageFailure {
    stage: CompilerStage,
    definition: Option<DefId>,
    source_file: Option<FileId>,
    diagnostics: Arc<[Diagnostic]>,
}

impl StageFailure {
    pub(super) fn new(stage: CompilerStage, mut diagnostics: Vec<Diagnostic>) -> Self {
        diagnostics.sort_by(|left, right| {
            left.span
                .file
                .cmp(&right.span.file)
                .then_with(|| left.span.start.cmp(&right.span.start))
                .then_with(|| left.code.cmp(right.code))
                .then_with(|| left.message.cmp(&right.message))
        });
        Self {
            stage,
            definition: None,
            source_file: None,
            diagnostics: diagnostics.into(),
        }
    }

    pub(super) fn one(stage: CompilerStage, diagnostic: Diagnostic) -> Self {
        Self::new(stage, vec![diagnostic])
    }

    pub(super) fn for_file(
        stage: CompilerStage,
        source_file: FileId,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let mut failure = Self::new(stage, diagnostics);
        failure.source_file = Some(source_file);
        failure
    }

    pub(super) fn for_definition(
        stage: CompilerStage,
        definition: DefId,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let mut failure = Self::new(stage, diagnostics);
        failure.definition = Some(definition);
        failure
    }

    pub(super) fn one_for_definition(
        stage: CompilerStage,
        definition: DefId,
        diagnostic: Diagnostic,
    ) -> Self {
        Self::for_definition(stage, definition, vec![diagnostic])
    }

    /// Stage which rejected the input.
    #[must_use]
    pub const fn stage(&self) -> CompilerStage {
        self.stage
    }

    /// Stable definition owning function-relative diagnostics, when present.
    #[must_use]
    pub const fn definition(&self) -> Option<DefId> {
        self.definition
    }

    /// Stable input file owning file-relative diagnostics, when present.
    #[must_use]
    pub const fn source_file(&self) -> Option<FileId> {
        self.source_file
    }

    /// Diagnostics in deterministic source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

pub(super) type StageResult<T> = Result<Arc<T>, Arc<StageFailure>>;

/// Current file location of a stable function definition.
///
/// Semantic stage products store function-relative provenance so an unrelated
/// edit before the declaration cannot turn its executable query products red.
/// This independently tracked anchor restores the current source location for
/// diagnostics and source-map publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionProvenance {
    logical_file: Arc<str>,
    byte_offset: usize,
    line: usize,
    column: usize,
}

impl FunctionProvenance {
    pub(super) fn new(
        logical_file: Arc<str>,
        byte_offset: usize,
        line: usize,
        column: usize,
    ) -> Self {
        Self {
            logical_file,
            byte_offset,
            line,
            column,
        }
    }

    #[must_use]
    pub fn logical_file(&self) -> &str {
        &self.logical_file
    }

    #[must_use]
    pub const fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }
}

/// One type-checked HIR function with function-relative source provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedHirFunction {
    function: Arc<hir::Function>,
    fingerprint: Fingerprint,
}

/// One exact typed function signature published independently of its body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedFunctionSignature {
    definition: DefId,
    signature: crate::compiler::types::Signature,
}

impl TypedFunctionSignature {
    pub(super) fn new(definition: DefId, signature: crate::compiler::types::Signature) -> Self {
        Self {
            definition,
            signature,
        }
    }

    #[must_use]
    pub const fn definition(&self) -> DefId {
        self.definition
    }

    #[must_use]
    pub const fn signature(&self) -> &crate::compiler::types::Signature {
        &self.signature
    }
}

impl TypedHirFunction {
    pub(super) fn new(function: Arc<hir::Function>) -> Self {
        let fingerprint = fingerprint::hir_function(&function);
        Self {
            function,
            fingerprint,
        }
    }

    #[must_use]
    pub fn function(&self) -> &hir::Function {
        &self.function
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

/// One Go MIR function after construction and package-ABI verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMirFunction {
    function: Arc<mir::Function>,
    fingerprint: Fingerprint,
}

impl VerifiedMirFunction {
    pub(super) fn new(function: mir::Function) -> Self {
        let fingerprint = fingerprint::mir_function(&function);
        Self {
            function: Arc::new(function),
            fingerprint,
        }
    }

    #[must_use]
    pub fn function(&self) -> &mir::Function {
        &self.function
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

/// One verified Go MIR function after mandatory semantic normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedMirFunction {
    function: Arc<mir::Function>,
    fingerprint: Fingerprint,
}

impl NormalizedMirFunction {
    pub(super) fn new(function: mir::Function) -> Self {
        let fingerprint = fingerprint::mir_function(&function);
        Self {
            function: Arc::new(function),
            fingerprint,
        }
    }

    #[must_use]
    pub fn function(&self) -> &mir::Function {
        &self.function
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

/// One verified explicit Rust representation function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRustIrFunction {
    function: Arc<rust_ir::Function>,
    fingerprint: Fingerprint,
    representation_key: Fingerprint,
}

impl VerifiedRustIrFunction {
    pub(super) fn new(function: rust_ir::Function, representation_key: Fingerprint) -> Self {
        let semantic = fingerprint::rust_ir_function(&function);
        let fingerprint = fingerprint_parts(
            b"configured-rust-ir-function",
            &[semantic.as_bytes(), representation_key.as_bytes()],
        );
        Self {
            function: Arc::new(function),
            fingerprint,
            representation_key,
        }
    }

    #[must_use]
    pub fn function(&self) -> &rust_ir::Function {
        &self.function
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    #[must_use]
    pub const fn representation_key(&self) -> Fingerprint {
        self.representation_key
    }
}

/// Complete verified Rust IR package assembled in stable definition order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRustIrPackage {
    file: Arc<rust_ir::File>,
    fingerprint: Fingerprint,
}

impl VerifiedRustIrPackage {
    pub(super) fn new(file: rust_ir::File, representation_key: Fingerprint) -> Self {
        let semantic = fingerprint::rust_ir_file(&file);
        Self {
            file: Arc::new(file),
            fingerprint: fingerprint_parts(
                b"configured-rust-ir-package",
                &[semantic.as_bytes(), representation_key.as_bytes()],
            ),
        }
    }

    #[must_use]
    pub fn file(&self) -> &rust_ir::File {
        &self.file
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct MirSignatureDependencies {
    pub(super) signatures: BTreeMap<DefId, crate::compiler::types::Signature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RustSignatureDependencies {
    pub(super) signatures: BTreeMap<DefId, rust_ir::Signature>,
    pub(super) representation_key: Fingerprint,
}
