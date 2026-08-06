//! Stable semantic identities and explicitly owner-local dense indexes.
//!
//! Cross-revision identities are content-addressed from structured keys and
//! interned in an invocation-owned table. A digest hit is never trusted on its
//! own: the interner retains and compares the complete key, turning a digest
//! collision into an explicit compiler error.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};

use super::input::{PackageKey, WorkspaceKey};

const IDENTITY_SCHEMA: &[u8] = b"gors-semantic-identity-v2";

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct StableFingerprint([u8; 32]);

impl StableFingerprint {
    const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for StableFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for StableFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Stable identity of a compiler workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceId(StableFingerprint);

/// Stable identity of a Go package within a workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageId(StableFingerprint);

/// Stable logical source-file identity within a package.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileId(StableFingerprint);

impl FileId {
    /// Canonical stable file bytes for compiler-owned encodings.
    pub(in crate::compiler) const fn canonical_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl PackageId {
    /// Canonical stable package bytes for compiler-owned encodings.
    pub(in crate::compiler) const fn canonical_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

/// Stable package-level definition identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefId(StableFingerprint);

impl DefId {
    /// Canonical stable definition bytes for compiler-owned encodings.
    pub(in crate::compiler) const fn canonical_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl fmt::Display for DefId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

/// Globally unambiguous identity of one package-owned definition.
///
/// [`DefId`] remains a package-owned key even though its current digest input
/// includes the declaring package. Cross-package products must retain this
/// explicit pair instead of attempting to recover or infer a [`PackageId`]
/// from opaque definition bytes. Canonical ordering is lexicographic by
/// package first and definition second.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QualifiedDefId {
    package: PackageId,
    definition: DefId,
}

impl QualifiedDefId {
    /// Pair an explicitly known package with one of its definition identities.
    #[must_use]
    pub const fn new(package: PackageId, definition: DefId) -> Self {
        Self {
            package,
            definition,
        }
    }

    /// Package that owns this definition.
    #[must_use]
    pub const fn package(self) -> PackageId {
        self.package
    }

    /// Package-local stable definition identity.
    #[must_use]
    pub const fn definition(self) -> DefId {
        self.definition
    }
}

impl fmt::Display for QualifiedDefId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}/{}", self.package, self.definition)
    }
}

/// Dense identity of one HIR node within a stable definition owner.
///
/// This pair is unique and deterministic for one rebuilt function, but the
/// dense component is deliberately not a persistent/query identity. Stable
/// node matching requires reusable incremental syntax identities, which the
/// bootstrap parser does not yet provide. Keeping the owner explicit prevents
/// accidental file-global use and keeps unrelated declaration ordering from
/// perturbing a function's rebuilt nodes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId {
    owner: DefId,
    local: u32,
}

impl NodeId {
    pub(super) fn owner_local(owner: DefId, local: u32) -> Self {
        Self { owner, local }
    }

    /// Stable definition that owns this dense node index.
    pub(in crate::compiler) const fn owner(self) -> DefId {
        self.owner
    }

    /// Dense index meaningful only within [`Self::owner`].
    pub(in crate::compiler) const fn local_index(self) -> u32 {
        self.local
    }
}

/// Dense index into one function's local table.
///
/// This is deliberately not a persistent/query identity. It is meaningful
/// only together with its owning [`DefId`] and may be renumbered whenever that
/// function is rebuilt.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalId(pub u32);

impl LocalId {
    /// Canonical owner-local index for compiler-owned encodings.
    pub(in crate::compiler) const fn index(self) -> u32 {
        self.0
    }
}

/// Dense identity of a named type declared inside one function.
///
/// This identity is deliberately revision-local and may only travel inside
/// its owning function's HIR and IR products. It is not a query or CAS key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalTypeId {
    owner: DefId,
    local: u32,
}

impl LocalTypeId {
    pub(super) const fn owner_local(owner: DefId, local: u32) -> Self {
        Self { owner, local }
    }

    pub(in crate::compiler) const fn owner(self) -> DefId {
        self.owner
    }

    pub(in crate::compiler) const fn local_index(self) -> u32 {
        self.local
    }
}

impl fmt::Display for LocalTypeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.owner, self.local)
    }
}

/// Dense identity of one non-escaping function literal within its owner.
///
/// Like [`LocalId`], this is revision-local and cannot be used as a query or
/// persistent cache key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClosureId(pub u32);

impl ClosureId {
    /// Canonical owner-local index for compiler-owned encodings.
    pub(in crate::compiler) const fn index(self) -> u32 {
        self.0
    }
}

/// Dense index into one MIR or Rust-IR function's block table.
///
/// Like [`LocalId`], this is an owner-local stage index, never a persistent
/// cross-revision query key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BasicBlockId(pub u32);

impl BasicBlockId {
    /// Canonical owner-local index for compiler-owned encodings.
    pub(in crate::compiler) const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DefinitionKind {
    Constant,
    Function,
    Type,
    Variable,
}

impl DefinitionKind {
    fn tag(self) -> &'static [u8] {
        match self {
            Self::Constant => b"constant",
            Self::Function => b"function",
            Self::Type => b"type",
            Self::Variable => b"variable",
        }
    }
}

/// Canonical identity of the named receiver type that owns a Go method set.
///
/// Pointer and value receiver syntax intentionally share this identity: Go
/// does not permit two methods with the same name on `T` and `*T`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReceiverIdentity {
    package: PackageId,
    declared_type: String,
}

impl ReceiverIdentity {
    pub fn named(package: PackageId, declared_type: impl Into<String>) -> Self {
        Self {
            package,
            declared_type: declared_type.into(),
        }
    }

    fn encode(&self, encoded: &mut CanonicalKey) {
        encoded.fingerprint(self.package.0);
        encoded.string(&self.declared_type);
    }
}

/// Namespace in which a definition name is unique.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DefinitionNamespace {
    Package,
    Method(ReceiverIdentity),
}

impl DefinitionNamespace {
    fn encode(&self, encoded: &mut CanonicalKey) {
        match self {
            Self::Package => encoded.bytes(b"package"),
            Self::Method(receiver) => {
                encoded.bytes(b"method");
                receiver.encode(encoded);
            }
        }
    }
}

/// Stable semantic discriminator for declarations that are not unique by
/// namespace, kind, and source name alone.
///
/// The supplied value must describe semantic identity (for example a stable
/// syntax-node identity once the parser has one). Traversal order, byte
/// offsets, and vector indexes are not valid discriminators.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DefinitionDisambiguator {
    None,
    Semantic(String),
}

impl DefinitionDisambiguator {
    fn encode(&self, encoded: &mut CanonicalKey) {
        match self {
            Self::None => encoded.bytes(b"none"),
            Self::Semantic(identity) => {
                encoded.bytes(b"semantic");
                encoded.string(identity);
            }
        }
    }
}

/// Complete persistent key for one package-owned Go definition.
///
/// The declaring file is deliberately absent. Moving a named declaration
/// between files in the same package must not perturb its identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefinitionKey {
    package: PackageId,
    namespace: DefinitionNamespace,
    kind: DefinitionKind,
    name: String,
    disambiguator: DefinitionDisambiguator,
}

impl DefinitionKey {
    pub fn package_named(
        package: PackageId,
        kind: DefinitionKind,
        name: impl Into<String>,
    ) -> Self {
        Self {
            package,
            namespace: DefinitionNamespace::Package,
            kind,
            name: name.into(),
            disambiguator: DefinitionDisambiguator::None,
        }
    }

    pub fn method(receiver: ReceiverIdentity, name: impl Into<String>) -> Self {
        Self {
            package: receiver.package,
            namespace: DefinitionNamespace::Method(receiver),
            kind: DefinitionKind::Function,
            name: name.into(),
            disambiguator: DefinitionDisambiguator::None,
        }
    }

    #[must_use]
    pub(crate) fn is_package_level(&self) -> bool {
        matches!(&self.namespace, DefinitionNamespace::Package)
    }

    pub fn disambiguated_package_definition(
        package: PackageId,
        kind: DefinitionKind,
        name: impl Into<String>,
        semantic_identity: impl Into<String>,
    ) -> Self {
        Self {
            package,
            namespace: DefinitionNamespace::Package,
            kind,
            name: name.into(),
            disambiguator: DefinitionDisambiguator::Semantic(semantic_identity.into()),
        }
    }

    /// Deterministic digest used as the compact cross-revision identity.
    ///
    /// Pure queries retain this full key and compare all keys sharing a digest
    /// at the package-index boundary. They must never trust this digest alone.
    pub fn id(&self) -> DefId {
        DefId(sha256_fingerprint(&self.encode()))
    }

    pub const fn package(&self) -> PackageId {
        self.package
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn encode(&self) -> Vec<u8> {
        let mut encoded = CanonicalKey::default();
        encoded.bytes(IDENTITY_SCHEMA);
        encoded.bytes(b"definition");
        encoded.fingerprint(self.package.0);
        self.namespace.encode(&mut encoded);
        encoded.bytes(self.kind.tag());
        encoded.string(&self.name);
        self.disambiguator.encode(&mut encoded);
        encoded.finish()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IdentityKey {
    Workspace {
        key: WorkspaceKey,
    },
    Package {
        workspace: WorkspaceId,
        key: PackageKey,
    },
    File {
        package: PackageId,
        logical_path: String,
    },
    #[cfg(test)]
    Definition(DefinitionKey),
}

impl IdentityKey {
    fn domain(&self) -> IdentityDomain {
        match self {
            Self::Workspace { .. } => IdentityDomain::Workspace,
            Self::Package { .. } => IdentityDomain::Package,
            Self::File { .. } => IdentityDomain::File,
            #[cfg(test)]
            Self::Definition(_) => IdentityDomain::Definition,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut encoded = CanonicalKey::default();
        encoded.bytes(IDENTITY_SCHEMA);
        match self {
            Self::Workspace { key } => {
                encoded.bytes(b"workspace");
                encoded.workspace_key(key);
            }
            Self::Package { workspace, key } => {
                encoded.bytes(b"package");
                encoded.fingerprint(workspace.0);
                encoded.package_key(key);
            }
            Self::File {
                package,
                logical_path,
            } => {
                encoded.bytes(b"file");
                encoded.fingerprint(package.0);
                encoded.string(logical_path);
            }
            #[cfg(test)]
            Self::Definition(key) => return key.encode(),
        }
        encoded.finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IdentityDomain {
    Workspace,
    Package,
    File,
    #[cfg(test)]
    Definition,
}

#[derive(Default)]
struct CanonicalKey {
    bytes: Vec<u8>,
}

impl CanonicalKey {
    fn bytes(&mut self, value: &[u8]) {
        let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
        self.bytes.extend_from_slice(&length.to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn fingerprint(&mut self, value: StableFingerprint) {
        self.bytes(&value.0);
    }

    fn workspace_key(&mut self, key: &WorkspaceKey) {
        match key {
            WorkspaceKey::Module(module_path) => {
                self.bytes(b"module");
                self.string(module_path.as_str());
            }
            WorkspaceKey::AdHoc(name) => {
                self.bytes(b"ad-hoc");
                self.string(name);
            }
        }
    }

    fn package_key(&mut self, key: &PackageKey) {
        match key {
            PackageKey::ImportPath(import_path) => {
                self.bytes(b"import-path");
                self.string(import_path.as_str());
            }
            PackageKey::CommandLine => self.bytes(b"command-line"),
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

type FingerprintFn = fn(&[u8]) -> StableFingerprint;

/// Compilation-owned collision-checking table for stable identities.
pub(crate) struct IdentityInterner {
    entries: BTreeMap<(IdentityDomain, StableFingerprint), IdentityKey>,
    fingerprint: FingerprintFn,
}

impl Default for IdentityInterner {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            fingerprint: sha256_fingerprint,
        }
    }
}

impl IdentityInterner {
    pub(crate) fn workspace(
        &mut self,
        key: &WorkspaceKey,
    ) -> Result<WorkspaceId, IdentityCollision> {
        self.intern(IdentityKey::Workspace { key: key.clone() })
            .map(WorkspaceId)
    }

    pub(crate) fn package(
        &mut self,
        workspace: WorkspaceId,
        key: &PackageKey,
    ) -> Result<PackageId, IdentityCollision> {
        self.intern(IdentityKey::Package {
            workspace,
            key: key.clone(),
        })
        .map(PackageId)
    }

    pub(crate) fn file(
        &mut self,
        package: PackageId,
        logical_path: impl Into<String>,
    ) -> Result<FileId, IdentityCollision> {
        self.intern(IdentityKey::File {
            package,
            logical_path: logical_path.into(),
        })
        .map(FileId)
    }

    #[cfg(test)]
    pub(crate) fn definition(&mut self, key: DefinitionKey) -> Result<DefId, IdentityCollision> {
        self.intern(IdentityKey::Definition(key)).map(DefId)
    }

    fn intern(&mut self, key: IdentityKey) -> Result<StableFingerprint, IdentityCollision> {
        let encoded = key.encode();
        let fingerprint = (self.fingerprint)(&encoded);
        let slot = (key.domain(), fingerprint);
        if let Some(existing) = self.entries.get(&slot) {
            if existing == &key {
                return Ok(fingerprint);
            }
            return Err(IdentityCollision {
                fingerprint,
                existing: Box::new(existing.clone()),
                requested: Box::new(key),
            });
        }
        self.entries.insert(slot, key);
        Ok(fingerprint)
    }

    #[cfg(test)]
    fn with_fingerprint(fingerprint: FingerprintFn) -> Self {
        Self {
            entries: BTreeMap::new(),
            fingerprint,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IdentityCollision {
    fingerprint: StableFingerprint,
    existing: Box<IdentityKey>,
    requested: Box<IdentityKey>,
}

impl fmt::Display for IdentityCollision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "stable identity digest collision for {}: existing full key {:?}, requested full key {:?}",
            self.fingerprint, self.existing, self.requested
        )
    }
}

impl std::error::Error for IdentityCollision {}

fn sha256_fingerprint(bytes: &[u8]) -> StableFingerprint {
    StableFingerprint(Sha256::digest(bytes).into())
}

#[cfg(test)]
mod tests;
