//! Compiler-owned query inputs and immutable projection products.

mod descriptors;
mod fingerprint_builder;
mod issues;
mod public_api;

pub use descriptors::{
    ConstantDescriptor, FunctionDescriptor, TypeAliasDescriptor, TypeDefinitionDescriptor,
    VariableDescriptor,
};
pub(super) use fingerprint_builder::FingerprintBuilder;
pub use issues::{FileIssue, PackageIssue};
pub use public_api::PublicApi;

use std::fmt;
use std::sync::Arc;

use gors_runtime_abi::{ContractIdentity, RuntimeAbiManifest};

use crate::compiler::syntax::{
    FunctionBodySyntax, FunctionHeaderSyntax, SemanticTokenStream, SyntaxAnchor,
};
use crate::source::TextRange;

use super::super::fingerprint::Fingerprint;
use super::super::ids::{DefId, FileId, PackageId};
use super::source_metadata::write_import_issue;

/// Compiler identity for the exact target-neutral runtime contract.
///
/// The runtime ABI crate owns canonical manifest encoding. Compiler queries
/// retain only its typed SHA-256 identity, never a label scraped from build
/// output or an ambient environment variable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeAbiId(ContractIdentity);

impl RuntimeAbiId {
    /// Construct an identity from a canonical runtime-contract digest.
    #[must_use]
    pub const fn from_contract_hash(bytes: [u8; 32]) -> Self {
        Self(ContractIdentity::from_bytes(bytes))
    }

    /// Identity of the current canonical runtime contract.
    #[must_use]
    pub fn current() -> Self {
        Self(RuntimeAbiManifest::current().identity())
    }

    /// Canonical contract identity used by artifact packaging.
    #[must_use]
    pub const fn contract_identity(self) -> ContractIdentity {
        self.0
    }

    /// Canonical digest bytes used by query and persistent-cache keys.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl From<ContractIdentity> for RuntimeAbiId {
    fn from(identity: ContractIdentity) -> Self {
        Self(identity)
    }
}

impl fmt::Display for RuntimeAbiId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

/// Configuration owned by one compiler database invocation.
///
/// The initial source-projection queries are target independent, so they do
/// not read this input yet. Later semantic and representation queries must
/// depend on the exact fields they consume rather than on ambient process
/// state.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BuildConfig {
    go_version: Arc<str>,
    runtime_abi: RuntimeAbiId,
}

impl BuildConfig {
    /// Construct an explicit compiler configuration.
    #[must_use]
    pub fn new(go_version: impl Into<Arc<str>>, runtime_abi: RuntimeAbiId) -> Self {
        Self {
            go_version: go_version.into(),
            runtime_abi,
        }
    }

    /// Pinned Go language/toolchain version.
    #[must_use]
    pub fn go_version(&self) -> &str {
        &self.go_version
    }

    /// Target-neutral runtime contract selected for representation lowering.
    #[must_use]
    pub const fn runtime_abi(&self) -> RuntimeAbiId {
        self.runtime_abi
    }

    /// Canonical, domain-separated content fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        let mut writer = FingerprintBuilder::new(b"build-config");
        writer.bytes(self.go_version.as_bytes());
        writer.bytes(self.runtime_abi.as_bytes());
        writer.finish()
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self::new(crate::GO_VERSION, RuntimeAbiId::current())
    }
}

/// Parser failure retained as an ordinary immutable query output.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ParseFailure {
    message: Arc<str>,
    physical_range: TextRange,
}

impl ParseFailure {
    pub(super) fn new(message: impl Into<Arc<str>>, physical_range: TextRange) -> Self {
        Self {
            message: message.into(),
            physical_range,
        }
    }

    /// Human-readable parser diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Exact zero-based physical byte anchor in the source revision.
    #[must_use]
    pub const fn physical_range(&self) -> TextRange {
        self.physical_range
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.message.len()
    }

    fn write_fingerprint(&self, writer: &mut FingerprintBuilder) {
        writer.bytes(self.message.as_bytes());
        writer.usize(self.physical_range.start().to_usize());
        writer.usize(self.physical_range.end().to_usize());
    }
}

/// Deterministic, body-independent index of one parsed source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileAnalysis {
    file: FileId,
    package: Arc<str>,
    functions: Arc<[FunctionDescriptor]>,
    constants: Arc<[ConstantDescriptor]>,
    variables: Arc<[VariableDescriptor]>,
    type_aliases: Arc<[TypeAliasDescriptor]>,
    type_definitions: Arc<[TypeDefinitionDescriptor]>,
    failure: Option<ParseFailure>,
    issues: Arc<[FileIssue]>,
    fingerprint: Fingerprint,
}

pub(super) struct FileAnalysisData {
    pub(super) file: FileId,
    pub(super) package: Arc<str>,
    pub(super) functions: Arc<[FunctionDescriptor]>,
    pub(super) constants: Arc<[ConstantDescriptor]>,
    pub(super) variables: Arc<[VariableDescriptor]>,
    pub(super) type_aliases: Arc<[TypeAliasDescriptor]>,
    pub(super) type_definitions: Arc<[TypeDefinitionDescriptor]>,
    pub(super) failure: Option<ParseFailure>,
    pub(super) issues: Arc<[FileIssue]>,
}

impl FileAnalysis {
    pub(super) fn new(data: FileAnalysisData) -> Self {
        let FileAnalysisData {
            file,
            package,
            functions,
            constants,
            variables,
            type_aliases,
            type_definitions,
            failure,
            issues,
        } = data;
        let mut writer = FingerprintBuilder::new(b"file-analysis");
        writer.bytes(file.canonical_bytes());
        writer.bytes(package.as_bytes());
        for function in &*functions {
            writer.bytes(function.id.canonical_bytes());
            writer.bytes(function.name.as_bytes());
        }
        for constant in &*constants {
            writer.bytes(constant.id.canonical_bytes());
            writer.bytes(constant.name.as_bytes());
        }
        for variable in &*variables {
            writer.bytes(variable.id.canonical_bytes());
            writer.bytes(variable.name.as_bytes());
        }
        for alias in &*type_aliases {
            writer.bytes(alias.id.canonical_bytes());
            writer.bytes(alias.name.as_bytes());
            writer.bytes(alias.target.as_bytes());
        }
        for definition in &*type_definitions {
            writer.bytes(definition.id.canonical_bytes());
            writer.bytes(definition.name.as_bytes());
            writer.bytes(definition.underlying.as_bytes());
        }
        if let Some(failure) = &failure {
            writer.bytes(b"parse-failure");
            failure.write_fingerprint(&mut writer);
        }
        for issue in &*issues {
            match issue {
                FileIssue::DuplicateDefinition(name) => {
                    writer.bytes(b"duplicate-definition");
                    writer.bytes(name.as_bytes());
                }
                FileIssue::FunctionProjectionFailure { name, message } => {
                    writer.bytes(b"function-projection-failure");
                    writer.bytes(name.as_bytes());
                    writer.bytes(message.as_bytes());
                }
                FileIssue::ConstantProjectionFailure { name, message } => {
                    writer.bytes(b"constant-projection-failure");
                    writer.bytes(name.as_bytes());
                    writer.bytes(message.as_bytes());
                }
                FileIssue::VariableProjectionFailure { name, message } => {
                    writer.bytes(b"variable-projection-failure");
                    writer.bytes(name.as_bytes());
                    writer.bytes(message.as_bytes());
                }
                FileIssue::TypeProjectionFailure { name, message } => {
                    writer.bytes(b"type-projection-failure");
                    writer.bytes(name.as_bytes());
                    writer.bytes(message.as_bytes());
                }
            }
        }
        Self {
            file,
            package,
            functions,
            constants,
            variables,
            type_aliases,
            type_definitions,
            failure,
            issues,
            fingerprint: writer.finish(),
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Parsed package-clause name, empty after a syntax failure.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Functions sorted by stable definition identity, not declaration order.
    #[must_use]
    pub fn functions(&self) -> &[FunctionDescriptor] {
        &self.functions
    }

    /// Constants sorted by stable definition identity.
    #[must_use]
    pub fn constants(&self) -> &[ConstantDescriptor] {
        &self.constants
    }

    /// Variables sorted by stable definition identity.
    #[must_use]
    pub fn variables(&self) -> &[VariableDescriptor] {
        &self.variables
    }

    /// Type aliases sorted by stable definition identity.
    #[must_use]
    pub fn type_aliases(&self) -> &[TypeAliasDescriptor] {
        &self.type_aliases
    }

    /// Defined types sorted by stable definition identity.
    #[must_use]
    pub fn type_definitions(&self) -> &[TypeDefinitionDescriptor] {
        &self.type_definitions
    }

    /// Parser diagnostic, if the input revision is invalid.
    #[must_use]
    pub const fn failure(&self) -> Option<&ParseFailure> {
        self.failure.as_ref()
    }

    /// Deterministically sorted indexing issues.
    #[must_use]
    pub fn issues(&self) -> &[FileIssue] {
        &self.issues
    }

    /// Canonical fingerprint of this body-independent index.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for future memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let functions = self.functions.iter().fold(0_usize, |total, function| {
            total.saturating_add(function.name.len())
        });
        let constants = self.constants.iter().fold(0_usize, |total, constant| {
            total.saturating_add(constant.name.len())
        });
        let variables = self.variables.iter().fold(0_usize, |total, variable| {
            total.saturating_add(variable.name.len())
        });
        let type_aliases = self.type_aliases.iter().fold(0_usize, |total, alias| {
            total
                .saturating_add(alias.name.len())
                .saturating_add(alias.target.len())
        });
        let type_definitions = self
            .type_definitions
            .iter()
            .fold(0_usize, |total, definition| {
                total
                    .saturating_add(definition.name.len())
                    .saturating_add(definition.underlying.len())
            });
        let issues = self.issues.iter().fold(0_usize, |total, issue| {
            let bytes = match issue {
                FileIssue::DuplicateDefinition(name) => name.len(),
                FileIssue::FunctionProjectionFailure { name, message }
                | FileIssue::ConstantProjectionFailure { name, message }
                | FileIssue::VariableProjectionFailure { name, message }
                | FileIssue::TypeProjectionFailure { name, message } => {
                    name.len().saturating_add(message.len())
                }
            };
            total.saturating_add(bytes)
        });
        self.package
            .len()
            .saturating_add(functions)
            .saturating_add(constants)
            .saturating_add(variables)
            .saturating_add(type_aliases)
            .saturating_add(type_definitions)
            .saturating_add(issues)
            .saturating_add(
                self.failure
                    .as_ref()
                    .map_or(0, ParseFailure::retained_bytes),
            )
            .saturating_add(32)
    }
}

/// Deterministic, body-independent semantic package index.
///
/// This is declaration projection data, not typed HIR. Its public API
/// fingerprint reads exported headers only, and therefore stays green across
/// private body edits and source-file insertion order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageAnalysis {
    package: PackageId,
    package_name: Arc<str>,
    files: Arc<[FileId]>,
    direct_imports: Arc<[Arc<str>]>,
    functions: Arc<[FunctionDescriptor]>,
    constants: Arc<[ConstantDescriptor]>,
    variables: Arc<[VariableDescriptor]>,
    type_aliases: Arc<[TypeAliasDescriptor]>,
    type_definitions: Arc<[TypeDefinitionDescriptor]>,
    issues: Arc<[PackageIssue]>,
    public_api_fingerprint: Fingerprint,
    fingerprint: Fingerprint,
}

pub(super) struct PackageAnalysisData {
    pub(super) package: PackageId,
    pub(super) package_name: Arc<str>,
    pub(super) files: Arc<[FileId]>,
    pub(super) direct_imports: Arc<[Arc<str>]>,
    pub(super) functions: Arc<[FunctionDescriptor]>,
    pub(super) constants: Arc<[ConstantDescriptor]>,
    pub(super) variables: Arc<[VariableDescriptor]>,
    pub(super) type_aliases: Arc<[TypeAliasDescriptor]>,
    pub(super) type_definitions: Arc<[TypeDefinitionDescriptor]>,
    pub(super) issues: Arc<[PackageIssue]>,
}

impl PackageAnalysis {
    pub(super) fn new(
        data: PackageAnalysisData,
        exported_signatures: &[FunctionSignature],
        exported_constants: &[(DefId, Fingerprint)],
        exported_variables: &[(DefId, Fingerprint)],
        exported_type_aliases: &[(DefId, Fingerprint)],
        exported_type_definitions: &[(DefId, Fingerprint)],
    ) -> Self {
        let PackageAnalysisData {
            package,
            package_name,
            files,
            direct_imports,
            functions,
            constants,
            variables,
            type_aliases,
            type_definitions,
            issues,
        } = data;
        let mut public_api = FingerprintBuilder::new(b"package-public-api");
        public_api.bytes(package.canonical_bytes());
        public_api.bytes(package_name.as_bytes());
        for signature in exported_signatures {
            public_api.bytes(signature.id.canonical_bytes());
            public_api.bytes(signature.fingerprint.as_bytes());
        }
        for (definition, fingerprint) in exported_constants {
            public_api.bytes(definition.canonical_bytes());
            public_api.bytes(fingerprint.as_bytes());
        }
        for (definition, fingerprint) in exported_variables {
            public_api.bytes(definition.canonical_bytes());
            public_api.bytes(fingerprint.as_bytes());
        }
        for (definition, fingerprint) in exported_type_aliases {
            public_api.bytes(definition.canonical_bytes());
            public_api.bytes(fingerprint.as_bytes());
        }
        for (definition, fingerprint) in exported_type_definitions {
            public_api.bytes(definition.canonical_bytes());
            public_api.bytes(fingerprint.as_bytes());
        }
        let public_api_fingerprint = public_api.finish();

        let mut fingerprint = FingerprintBuilder::new(b"package-analysis");
        fingerprint.bytes(package.canonical_bytes());
        fingerprint.bytes(package_name.as_bytes());
        for file in &*files {
            fingerprint.bytes(file.canonical_bytes());
        }
        for import in &*direct_imports {
            fingerprint.bytes(import.as_bytes());
        }
        for function in &*functions {
            fingerprint.bytes(function.id.canonical_bytes());
            fingerprint.bytes(function.file.canonical_bytes());
            fingerprint.bytes(function.name.as_bytes());
        }
        for constant in &*constants {
            fingerprint.bytes(constant.id.canonical_bytes());
            fingerprint.bytes(constant.file.canonical_bytes());
            fingerprint.bytes(constant.name.as_bytes());
        }
        for variable in &*variables {
            fingerprint.bytes(variable.id.canonical_bytes());
            fingerprint.bytes(variable.file.canonical_bytes());
            fingerprint.bytes(variable.name.as_bytes());
        }
        for alias in &*type_aliases {
            fingerprint.bytes(alias.id.canonical_bytes());
            fingerprint.bytes(alias.file.canonical_bytes());
            fingerprint.bytes(alias.name.as_bytes());
            fingerprint.bytes(alias.target.as_bytes());
        }
        for definition in &*type_definitions {
            fingerprint.bytes(definition.id.canonical_bytes());
            fingerprint.bytes(definition.file.canonical_bytes());
            fingerprint.bytes(definition.name.as_bytes());
            fingerprint.bytes(definition.underlying.as_bytes());
        }
        for issue in &*issues {
            match issue {
                PackageIssue::FileParseFailure { file, failure } => {
                    fingerprint.bytes(b"file-parse-failure");
                    fingerprint.bytes(file.canonical_bytes());
                    failure.write_fingerprint(&mut fingerprint);
                }
                PackageIssue::InvalidImportPath {
                    file,
                    literal,
                    line,
                    column,
                    virtual_file,
                    issue,
                } => {
                    fingerprint.bytes(b"invalid-import-path");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(literal.as_bytes());
                    fingerprint.usize(*line);
                    fingerprint.usize(*column);
                    match virtual_file {
                        Some(file) => {
                            fingerprint.bytes(b"virtual-file");
                            fingerprint.bytes(file.as_bytes());
                        }
                        None => fingerprint.bytes(b"physical-file"),
                    }
                    write_import_issue(&mut fingerprint, issue);
                }
                PackageIssue::PackageClauseMismatch {
                    file,
                    expected,
                    found,
                } => {
                    fingerprint.bytes(b"package-clause-mismatch");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(expected.as_bytes());
                    fingerprint.bytes(found.as_bytes());
                }
                PackageIssue::DuplicateDefinition {
                    name,
                    first_file,
                    second_file,
                } => {
                    fingerprint.bytes(b"duplicate-definition");
                    fingerprint.bytes(name.as_bytes());
                    fingerprint.bytes(first_file.canonical_bytes());
                    fingerprint.bytes(second_file.canonical_bytes());
                }
                PackageIssue::IdentityCollision {
                    id,
                    existing_key,
                    requested_key,
                } => {
                    fingerprint.bytes(b"identity-collision");
                    fingerprint.bytes(id.canonical_bytes());
                    fingerprint.bytes(existing_key.as_bytes());
                    fingerprint.bytes(requested_key.as_bytes());
                }
                PackageIssue::FunctionProjectionFailure {
                    file,
                    name,
                    message,
                } => {
                    fingerprint.bytes(b"function-projection-failure");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(name.as_bytes());
                    fingerprint.bytes(message.as_bytes());
                }
                PackageIssue::ConstantProjectionFailure {
                    file,
                    name,
                    message,
                } => {
                    fingerprint.bytes(b"constant-projection-failure");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(name.as_bytes());
                    fingerprint.bytes(message.as_bytes());
                }
                PackageIssue::VariableProjectionFailure {
                    file,
                    name,
                    message,
                } => {
                    fingerprint.bytes(b"variable-projection-failure");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(name.as_bytes());
                    fingerprint.bytes(message.as_bytes());
                }
                PackageIssue::TypeProjectionFailure {
                    file,
                    name,
                    message,
                } => {
                    fingerprint.bytes(b"type-projection-failure");
                    fingerprint.bytes(file.canonical_bytes());
                    fingerprint.bytes(name.as_bytes());
                    fingerprint.bytes(message.as_bytes());
                }
            }
        }
        fingerprint.bytes(public_api_fingerprint.as_bytes());

        Self {
            package,
            package_name,
            files,
            direct_imports,
            functions,
            constants,
            variables,
            type_aliases,
            type_definitions,
            issues,
            public_api_fingerprint,
            fingerprint: fingerprint.finish(),
        }
    }

    /// Stable canonical package identity.
    #[must_use]
    pub const fn package(&self) -> PackageId {
        self.package
    }

    /// Package-clause name selected from sorted valid file inputs.
    #[must_use]
    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    /// Stable source-file identities in deterministic order.
    #[must_use]
    pub fn files(&self) -> &[FileId] {
        &self.files
    }

    /// Canonical direct import paths, sorted and deduplicated package-wide.
    #[must_use]
    pub fn direct_imports(&self) -> &[Arc<str>] {
        &self.direct_imports
    }

    /// All indexed function declarations in stable key and evidence order.
    #[must_use]
    pub fn functions(&self) -> &[FunctionDescriptor] {
        &self.functions
    }

    /// All indexed package constants in stable key and evidence order.
    #[must_use]
    pub fn constants(&self) -> &[ConstantDescriptor] {
        &self.constants
    }

    /// All indexed package variables in stable key and evidence order.
    #[must_use]
    pub fn variables(&self) -> &[VariableDescriptor] {
        &self.variables
    }

    /// All indexed package type aliases in stable key and evidence order.
    #[must_use]
    pub fn type_aliases(&self) -> &[TypeAliasDescriptor] {
        &self.type_aliases
    }

    /// All indexed defined types in stable key and evidence order.
    #[must_use]
    pub fn type_definitions(&self) -> &[TypeDefinitionDescriptor] {
        &self.type_definitions
    }

    /// Deterministically sorted package-index issues.
    #[must_use]
    pub fn issues(&self) -> &[PackageIssue] {
        &self.issues
    }

    /// Fingerprint of exported headers only.
    #[must_use]
    pub const fn public_api_fingerprint(&self) -> Fingerprint {
        self.public_api_fingerprint
    }

    /// Fingerprint of the complete body-independent package index.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let functions = self.functions.iter().fold(0_usize, |total, function| {
            total.saturating_add(function.name.len())
        });
        let constants = self.constants.iter().fold(0_usize, |total, constant| {
            total.saturating_add(constant.name.len())
        });
        let variables = self.variables.iter().fold(0_usize, |total, variable| {
            total.saturating_add(variable.name.len())
        });
        let type_aliases = self.type_aliases.iter().fold(0_usize, |total, alias| {
            total
                .saturating_add(alias.name.len())
                .saturating_add(alias.target.len())
        });
        let type_definitions = self
            .type_definitions
            .iter()
            .fold(0_usize, |total, definition| {
                total
                    .saturating_add(definition.name.len())
                    .saturating_add(definition.underlying.len())
            });
        let issues = self.issues.iter().fold(0_usize, |total, issue| {
            let retained = match issue {
                PackageIssue::FileParseFailure { failure, .. } => failure.retained_bytes(),
                PackageIssue::InvalidImportPath {
                    literal,
                    virtual_file,
                    ..
                } => literal
                    .len()
                    .saturating_add(virtual_file.as_ref().map_or(0, |file| file.len())),
                PackageIssue::PackageClauseMismatch {
                    expected, found, ..
                } => expected.len().saturating_add(found.len()),
                PackageIssue::DuplicateDefinition { name, .. } => name.len(),
                PackageIssue::IdentityCollision {
                    existing_key,
                    requested_key,
                    ..
                } => existing_key.len().saturating_add(requested_key.len()),
                PackageIssue::FunctionProjectionFailure { name, message, .. }
                | PackageIssue::ConstantProjectionFailure { name, message, .. }
                | PackageIssue::VariableProjectionFailure { name, message, .. }
                | PackageIssue::TypeProjectionFailure { name, message, .. } => {
                    name.len().saturating_add(message.len())
                }
            };
            total.saturating_add(retained)
        });
        self.package_name
            .len()
            .saturating_add(self.files.len().saturating_mul(32))
            .saturating_add(
                self.direct_imports
                    .iter()
                    .fold(0_usize, |total, import| total.saturating_add(import.len())),
            )
            .saturating_add(functions)
            .saturating_add(constants)
            .saturating_add(variables)
            .saturating_add(type_aliases)
            .saturating_add(type_definitions)
            .saturating_add(issues)
            .saturating_add(64)
    }
}

/// Owned, trivia-insensitive structural function-header projection.
///
/// This is deliberately smaller than the parser AST and excludes physical
/// layout. It is not yet a type-checked Go signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionSignature {
    id: DefId,
    name: Arc<str>,
    anchor: SyntaxAnchor,
    syntax: SemanticTokenStream,
    structure: Arc<FunctionHeaderSyntax>,
    has_parameters: bool,
    has_results: bool,
    fingerprint: Fingerprint,
}

impl FunctionSignature {
    pub(super) fn new(
        id: DefId,
        name: Arc<str>,
        anchor: SyntaxAnchor,
        syntax: SemanticTokenStream,
        structure: Arc<FunctionHeaderSyntax>,
        has_parameters: bool,
        has_results: bool,
    ) -> Self {
        let mut writer = FingerprintBuilder::new(b"function-signature-syntax-v1");
        writer.bytes(id.canonical_bytes());
        writer.bytes(name.as_bytes());
        writer.bytes(anchor.fingerprint().as_bytes());
        writer.bytes(syntax.fingerprint().as_bytes());
        writer.bytes(if has_parameters {
            b"parameters-present"
        } else {
            b"parameters-empty"
        });
        writer.bytes(if has_results {
            b"results-present"
        } else {
            b"results-empty"
        });
        Self {
            id,
            name,
            anchor,
            syntax,
            structure,
            has_parameters,
            has_results,
            fingerprint: writer.finish(),
        }
    }

    /// Stable definition identity.
    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    /// Go function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stable declaration anchor independent of source layout.
    #[must_use]
    pub const fn anchor(&self) -> &SyntaxAnchor {
        &self.anchor
    }

    /// Canonical owned header tokens.
    #[must_use]
    pub const fn syntax(&self) -> &SemanticTokenStream {
        &self.syntax
    }

    /// Owned structural header consumed by semantic lowering.
    #[must_use]
    pub fn structure(&self) -> &FunctionHeaderSyntax {
        &self.structure
    }

    /// Whether the Go declaration contains one or more parameters.
    #[must_use]
    pub const fn has_parameters(&self) -> bool {
        self.has_parameters
    }

    /// Whether the Go declaration contains one or more results.
    #[must_use]
    pub const fn has_results(&self) -> bool {
        self.has_results
    }

    /// Domain-separated header fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for future memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.name
            .len()
            .saturating_add(self.syntax.retained_bytes())
            .saturating_add(32)
    }
}

/// Owned, trivia-insensitive function-body projection.
///
/// Physical layout is retained separately. This is semantic source projection
/// data, not typed HIR or executable MIR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionBody {
    id: DefId,
    anchor: SyntaxAnchor,
    syntax: Option<SemanticTokenStream>,
    structure: Arc<FunctionBodySyntax>,
    fingerprint: Fingerprint,
}

impl FunctionBody {
    pub(super) fn new(
        id: DefId,
        anchor: SyntaxAnchor,
        syntax: Option<SemanticTokenStream>,
        structure: Arc<FunctionBodySyntax>,
    ) -> Self {
        let mut writer = FingerprintBuilder::new(b"function-body-syntax-v1");
        writer.bytes(id.canonical_bytes());
        writer.bytes(anchor.fingerprint().as_bytes());
        match &syntax {
            Some(syntax) => {
                writer.bytes(b"present");
                writer.bytes(syntax.fingerprint().as_bytes());
            }
            None => writer.bytes(b"absent"),
        }
        Self {
            id,
            anchor,
            syntax,
            structure,
            fingerprint: writer.finish(),
        }
    }

    /// Stable definition identity.
    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    /// Stable declaration anchor independent of source layout.
    #[must_use]
    pub const fn anchor(&self) -> &SyntaxAnchor {
        &self.anchor
    }

    /// Canonical owned braced-body tokens, absent for bodyless declarations.
    #[must_use]
    pub const fn syntax(&self) -> Option<&SemanticTokenStream> {
        self.syntax.as_ref()
    }

    /// Owned structural body consumed by semantic lowering.
    #[must_use]
    pub fn structure(&self) -> &FunctionBodySyntax {
        &self.structure
    }

    /// Domain-separated body fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for future memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.syntax
            .as_ref()
            .map_or(32, |syntax| syntax.retained_bytes().saturating_add(32))
    }
}
