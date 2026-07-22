//! Compiler-owned query inputs and immutable projection products.

use std::fmt;
use std::sync::Arc;

use gors_runtime_abi::{ContractIdentity, RuntimeAbiManifest};

use crate::parser::ImportPathIssue;
use crate::source::TextRange;

use super::super::fingerprint::{Fingerprint, fingerprint_parts};
use super::super::ids::{DefId, DefinitionKey, FileId, PackageId};
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
    target: Arc<str>,
    go_version: Arc<str>,
    runtime_abi: RuntimeAbiId,
}

impl BuildConfig {
    /// Construct an explicit compiler configuration.
    #[must_use]
    pub fn new(
        target: impl Into<Arc<str>>,
        go_version: impl Into<Arc<str>>,
        runtime_abi: RuntimeAbiId,
    ) -> Self {
        Self {
            target: target.into(),
            go_version: go_version.into(),
            runtime_abi,
        }
    }

    /// Explicit target identity; never inferred from the host environment.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
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
        writer.bytes(self.target.as_bytes());
        writer.bytes(self.go_version.as_bytes());
        writer.bytes(self.runtime_abi.as_bytes());
        writer.finish()
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self::new("rust-source", crate::GO_VERSION, RuntimeAbiId::current())
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

/// Non-syntax issue discovered while indexing declarations.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FileIssue {
    /// A Go file declares the same free-function name more than once.
    DuplicateFunction(Arc<str>),
}

/// Stable function identity and display name in one file index.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FunctionDescriptor {
    id: DefId,
    file: FileId,
    name: Arc<str>,
    key: DefinitionKey,
}

impl FunctionDescriptor {
    pub(super) fn new(file: FileId, key: DefinitionKey, name: Arc<str>) -> Self {
        Self {
            id: key.id(),
            file,
            name,
            key,
        }
    }

    /// Stable cross-revision definition identity.
    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    /// Stable logical file containing this declaration revision.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Go declaration name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Deterministic, body-independent index of one parsed source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileAnalysis {
    file: FileId,
    package: Arc<str>,
    functions: Arc<[FunctionDescriptor]>,
    failure: Option<ParseFailure>,
    issues: Arc<[FileIssue]>,
    fingerprint: Fingerprint,
}

impl FileAnalysis {
    pub(super) fn new(
        file: FileId,
        package: Arc<str>,
        functions: Arc<[FunctionDescriptor]>,
        failure: Option<ParseFailure>,
        issues: Arc<[FileIssue]>,
    ) -> Self {
        let mut writer = FingerprintBuilder::new(b"file-analysis");
        writer.bytes(file.canonical_bytes());
        writer.bytes(package.as_bytes());
        for function in &*functions {
            writer.bytes(function.id.canonical_bytes());
            writer.bytes(function.name.as_bytes());
        }
        if let Some(failure) = &failure {
            writer.bytes(b"parse-failure");
            failure.write_fingerprint(&mut writer);
        }
        for issue in &*issues {
            match issue {
                FileIssue::DuplicateFunction(name) => {
                    writer.bytes(b"duplicate-function");
                    writer.bytes(name.as_bytes());
                }
            }
        }
        Self {
            file,
            package,
            functions,
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
        let issues = self.issues.iter().fold(0_usize, |total, issue| {
            let bytes = match issue {
                FileIssue::DuplicateFunction(name) => name.len(),
            };
            total.saturating_add(bytes)
        });
        self.package
            .len()
            .saturating_add(functions)
            .saturating_add(issues)
            .saturating_add(
                self.failure
                    .as_ref()
                    .map_or(0, ParseFailure::retained_bytes),
            )
            .saturating_add(32)
    }
}

/// Deterministic package-index issue, distinct from stable-ID interning.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PackageIssue {
    /// One package input file currently has invalid Go syntax.
    FileParseFailure { file: FileId, failure: ParseFailure },
    /// One import literal cannot name a canonical Go package.
    InvalidImportPath {
        file: FileId,
        literal: Arc<str>,
        line: usize,
        column: usize,
        virtual_file: Option<Arc<str>>,
        issue: ImportPathIssue,
    },
    /// Independently parsed files disagree on their package clause.
    PackageClauseMismatch {
        file: FileId,
        expected: Arc<str>,
        found: Arc<str>,
    },
    /// The same package-level name was declared by two source files.
    DuplicateDefinition {
        name: Arc<str>,
        first_file: FileId,
        second_file: FileId,
    },
    /// Distinct complete definition keys produced the same compact digest.
    IdentityCollision {
        id: DefId,
        existing_key: Arc<str>,
        requested_key: Arc<str>,
    },
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
    issues: Arc<[PackageIssue]>,
    public_api_fingerprint: Fingerprint,
    fingerprint: Fingerprint,
}

impl PackageAnalysis {
    pub(super) fn new(
        package: PackageId,
        package_name: Arc<str>,
        files: Arc<[FileId]>,
        direct_imports: Arc<[Arc<str>]>,
        functions: Arc<[FunctionDescriptor]>,
        issues: Arc<[PackageIssue]>,
        exported_signatures: &[FunctionSignature],
    ) -> Self {
        let mut public_api = FingerprintBuilder::new(b"package-public-api");
        public_api.bytes(package.canonical_bytes());
        public_api.bytes(package_name.as_bytes());
        for signature in exported_signatures {
            public_api.bytes(signature.id.canonical_bytes());
            public_api.bytes(signature.fingerprint.as_bytes());
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
            }
        }
        fingerprint.bytes(public_api_fingerprint.as_bytes());

        Self {
            package,
            package_name,
            files,
            direct_imports,
            functions,
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
            .saturating_add(issues)
            .saturating_add(64)
    }
}

/// Structural function-header projection.
///
/// `source` is the exact declaration header from `func` through the byte
/// before the body brace. This bootstrap representation is intentionally
/// trivia-sensitive; it is not yet a type-checked Go signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionSignature {
    id: DefId,
    name: Arc<str>,
    source: Arc<str>,
    has_parameters: bool,
    has_results: bool,
    fingerprint: Fingerprint,
}

impl FunctionSignature {
    pub(super) fn new(
        id: DefId,
        name: Arc<str>,
        source: Arc<str>,
        has_parameters: bool,
        has_results: bool,
    ) -> Self {
        let mut writer = FingerprintBuilder::new(b"function-signature-source");
        writer.bytes(id.canonical_bytes());
        writer.bytes(name.as_bytes());
        writer.bytes(source.as_bytes());
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
            source,
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

    /// Exact header source retained by this projection.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
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
            .saturating_add(self.source.len())
            .saturating_add(32)
    }
}

/// Structural function-body projection, separate from its public header.
///
/// This is source projection data, not typed HIR or executable MIR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionBody {
    id: DefId,
    source: Option<Arc<str>>,
    fingerprint: Fingerprint,
}

impl FunctionBody {
    pub(super) fn new(id: DefId, source: Option<Arc<str>>) -> Self {
        let mut writer = FingerprintBuilder::new(b"function-body-source");
        writer.bytes(id.canonical_bytes());
        match &source {
            Some(source) => {
                writer.bytes(b"present");
                writer.bytes(source.as_bytes());
            }
            None => writer.bytes(b"absent"),
        }
        Self {
            id,
            source,
            fingerprint: writer.finish(),
        }
    }

    /// Stable definition identity.
    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    /// Exact braced body source, or `None` for a bodyless declaration.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Domain-separated body fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for future memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.source
            .as_ref()
            .map_or(32, |source| source.len().saturating_add(32))
    }
}

/// Public-header aggregate for one source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicApi {
    file: FileId,
    signatures: Arc<[FunctionSignature]>,
    fingerprint: Fingerprint,
}

impl PublicApi {
    pub(super) fn new(file: FileId, signatures: Arc<[FunctionSignature]>) -> Self {
        let mut writer = FingerprintBuilder::new(b"file-public-api");
        writer.bytes(file.canonical_bytes());
        for signature in &*signatures {
            writer.bytes(signature.name.as_bytes());
            writer.bytes(signature.fingerprint.as_bytes());
        }
        Self {
            file,
            signatures,
            fingerprint: writer.finish(),
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Function headers sorted by stable definition identity.
    #[must_use]
    pub fn signatures(&self) -> &[FunctionSignature] {
        &self.signatures
    }

    /// Domain-separated aggregate fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for future memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.signatures.iter().fold(32_usize, |total, signature| {
            total.saturating_add(signature.retained_bytes())
        })
    }
}

pub(super) struct FingerprintBuilder {
    domain: Vec<u8>,
    parts: Vec<Vec<u8>>,
}

impl FingerprintBuilder {
    pub(super) fn new(domain: &[u8]) -> Self {
        Self {
            domain: domain.to_vec(),
            parts: Vec::new(),
        }
    }

    pub(super) fn bytes(&mut self, value: &[u8]) {
        self.parts.push(value.to_vec());
    }

    pub(super) fn usize(&mut self, value: usize) {
        self.bytes(&u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
    }

    pub(super) fn finish(self) -> Fingerprint {
        let parts = self.parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
        fingerprint_parts(&self.domain, &parts)
    }
}
