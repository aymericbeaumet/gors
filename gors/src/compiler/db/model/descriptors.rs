//! Stable package-level declaration descriptors used by file and package indexes.

use std::sync::Arc;

use crate::compiler::ids::{DefId, DefinitionKey, FileId};

/// Stable function identity and display name in one file index.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FunctionDescriptor {
    pub(super) id: DefId,
    pub(super) file: FileId,
    pub(super) name: Arc<str>,
    key: DefinitionKey,
}

impl FunctionDescriptor {
    pub(in crate::compiler::db) fn new(file: FileId, key: DefinitionKey, name: Arc<str>) -> Self {
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

/// Stable package constant identity and display name in one file index.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConstantDescriptor {
    pub(super) id: DefId,
    pub(super) file: FileId,
    pub(super) name: Arc<str>,
    key: DefinitionKey,
}

impl ConstantDescriptor {
    pub(in crate::compiler::db) fn new(file: FileId, key: DefinitionKey, name: Arc<str>) -> Self {
        Self {
            id: key.id(),
            file,
            name,
            key,
        }
    }

    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Stable package type-alias identity and target name in one file index.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeAliasDescriptor {
    pub(super) id: DefId,
    pub(super) file: FileId,
    pub(super) name: Arc<str>,
    pub(super) target: Arc<str>,
    key: DefinitionKey,
}

impl TypeAliasDescriptor {
    pub(in crate::compiler::db) fn new(
        file: FileId,
        key: DefinitionKey,
        name: Arc<str>,
        target: Arc<str>,
    ) -> Self {
        Self {
            id: key.id(),
            file,
            name,
            target,
            key,
        }
    }

    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }
}

/// Stable defined-type identity and underlying type name in one file index.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeDefinitionDescriptor {
    pub(super) id: DefId,
    pub(super) file: FileId,
    pub(super) name: Arc<str>,
    pub(super) underlying: Arc<str>,
    key: DefinitionKey,
}

impl TypeDefinitionDescriptor {
    pub(in crate::compiler::db) fn new(
        file: FileId,
        key: DefinitionKey,
        name: Arc<str>,
        underlying: Arc<str>,
    ) -> Self {
        Self {
            id: key.id(),
            file,
            name,
            underlying,
            key,
        }
    }

    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }
}
