//! Compiler-owned source provenance.
//!
//! Semantic products retain compact [`SourceRef`] values. A separately tracked
//! [`DefinitionSourceTable`] resolves those references to physical byte ranges
//! for one source revision. Paths and adjusted `//line` coordinates never enter
//! either representation.

use std::collections::BTreeMap;
use std::fmt;

use crate::source::{TextRange, TextSize};

use super::ids::{DefId, FileId, LocalId, NodeId};

/// A physical byte range paired with its stable compiler file identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileRange {
    file: FileId,
    range: TextRange,
}

impl FileRange {
    #[must_use]
    pub const fn new(file: FileId, range: TextRange) -> Self {
        Self { file, range }
    }

    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn range(self) -> TextRange {
        self.range
    }
}

/// Compact provenance retained by HIR and later semantic stages.
///
/// A definition reference is persistent because its [`DefId`] is persistent.
/// Node and local references use the compiler's current owner-local identities;
/// they must not become independent query or CAS keys until the syntax layer
/// supplies reusable anchors.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceRef {
    owner: DefId,
    slot: SourceSlot,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum SourceSlot {
    Definition,
    Node(u32),
    Local(LocalId),
}

/// Public structural view of a [`SourceRef`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceRefKind {
    Definition,
    Node(NodeId),
    Local(LocalId),
}

impl SourceRef {
    /// Reference the declaration owned by `owner`.
    #[must_use]
    pub const fn definition(owner: DefId) -> Self {
        Self {
            owner,
            slot: SourceSlot::Definition,
        }
    }

    /// Reference an owner-local HIR node.
    #[must_use]
    pub const fn node(node: NodeId) -> Self {
        Self {
            owner: node.owner(),
            slot: SourceSlot::Node(node.local_index()),
        }
    }

    /// Reference an owner-local binding.
    #[must_use]
    pub const fn local(owner: DefId, local: LocalId) -> Self {
        Self {
            owner,
            slot: SourceSlot::Local(local),
        }
    }

    /// Stable definition that owns this source reference.
    #[must_use]
    pub const fn owner(self) -> DefId {
        self.owner
    }

    /// Structural kind and owner-local identity of this reference.
    #[must_use]
    pub fn kind(self) -> SourceRefKind {
        match self.slot {
            SourceSlot::Definition => SourceRefKind::Definition,
            SourceSlot::Node(local) => SourceRefKind::Node(NodeId::owner_local(self.owner, local)),
            SourceSlot::Local(local) => SourceRefKind::Local(local),
        }
    }
}

/// Physical source ranges for exactly one semantic definition and revision.
///
/// Dense node and local slots are stored in index order. Consequently equality,
/// ordering, and hashing are independent of the order in which mappings were
/// supplied to [`Self::try_new`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefinitionSourceTable {
    owner: DefId,
    file: FileId,
    source_len: TextSize,
    definition: FileRange,
    nodes: Box<[FileRange]>,
    locals: Box<[FileRange]>,
}

impl DefinitionSourceTable {
    /// Validate and construct one complete per-definition source table.
    ///
    /// Mappings may arrive in any order. They must contain the definition
    /// reference exactly once, use this table's owner and file, fit within the
    /// source revision, and populate node and local indexes densely from zero.
    ///
    /// # Errors
    ///
    /// Returns [`SourceTableBuildError`] when any mapping violates those
    /// invariants or when a required dense slot is absent.
    pub fn try_new(
        owner: DefId,
        file: FileId,
        source_len: TextSize,
        mappings: impl IntoIterator<Item = (SourceRef, FileRange)>,
    ) -> Result<Self, SourceTableBuildError> {
        let mut mappings = mappings.into_iter().collect::<Vec<_>>();
        mappings.sort_unstable();

        let mut definition = None;
        let mut nodes = BTreeMap::new();
        let mut locals = BTreeMap::new();

        for (source_ref, file_range) in mappings {
            if source_ref.owner() != owner {
                return Err(SourceTableBuildError::OwnerMismatch {
                    table_owner: owner,
                    source_ref,
                });
            }
            if file_range.file() != file {
                return Err(SourceTableBuildError::FileMismatch {
                    source_ref,
                    expected: file,
                    actual: file_range.file(),
                });
            }
            if file_range.range().end() > source_len {
                return Err(SourceTableBuildError::RangeOutOfBounds {
                    source_ref,
                    range: file_range.range(),
                    source_len,
                });
            }

            let duplicate = match source_ref.slot {
                SourceSlot::Definition => definition.replace(file_range).is_some(),
                SourceSlot::Node(index) => nodes.insert(index, file_range).is_some(),
                SourceSlot::Local(local) => locals.insert(local.index(), file_range).is_some(),
            };
            if duplicate {
                return Err(SourceTableBuildError::DuplicateReference { source_ref });
            }
        }

        let definition = definition.ok_or(SourceTableBuildError::MissingDefinition { owner })?;
        let nodes = dense_ranges(owner, SourceTableSlotKind::Node, nodes)?;
        let locals = dense_ranges(owner, SourceTableSlotKind::Local, locals)?;

        Ok(Self {
            owner,
            file,
            source_len,
            definition,
            nodes,
            locals,
        })
    }

    #[must_use]
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn source_len(&self) -> TextSize {
        self.source_len
    }

    /// Resolve a reference from this definition to its physical file range.
    ///
    /// # Errors
    ///
    /// Returns [`SourceLookupError`] when the reference belongs to another
    /// definition or names a node or local outside this table.
    pub fn resolve(&self, source_ref: SourceRef) -> Result<FileRange, SourceLookupError> {
        if source_ref.owner() != self.owner {
            return Err(SourceLookupError::OwnerMismatch {
                table_owner: self.owner,
                source_ref,
            });
        }

        match source_ref.slot {
            SourceSlot::Definition => Ok(self.definition),
            SourceSlot::Node(index) => usize::try_from(index)
                .ok()
                .and_then(|index| self.nodes.get(index))
                .copied()
                .ok_or(SourceLookupError::MissingNode {
                    owner: self.owner,
                    index,
                }),
            SourceSlot::Local(local) => usize::try_from(local.index())
                .ok()
                .and_then(|index| self.locals.get(index))
                .copied()
                .ok_or(SourceLookupError::MissingLocal {
                    owner: self.owner,
                    local,
                }),
        }
    }
}

/// Dense source-table domain reported by construction failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceTableSlotKind {
    Node,
    Local,
}

fn dense_ranges(
    owner: DefId,
    kind: SourceTableSlotKind,
    ranges: BTreeMap<u32, FileRange>,
) -> Result<Box<[FileRange]>, SourceTableBuildError> {
    let mut dense = Vec::with_capacity(ranges.len());
    for (actual, range) in ranges {
        let expected = u32::try_from(dense.len())
            .map_err(|_| SourceTableBuildError::DenseIndexOverflow { owner, kind })?;
        if actual != expected {
            return Err(SourceTableBuildError::MissingDenseSlot {
                owner,
                kind,
                index: expected,
            });
        }
        dense.push(range);
    }
    Ok(dense.into_boxed_slice())
}

/// Invalid input supplied while constructing a [`DefinitionSourceTable`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceTableBuildError {
    OwnerMismatch {
        table_owner: DefId,
        source_ref: SourceRef,
    },
    FileMismatch {
        source_ref: SourceRef,
        expected: FileId,
        actual: FileId,
    },
    RangeOutOfBounds {
        source_ref: SourceRef,
        range: TextRange,
        source_len: TextSize,
    },
    DuplicateReference {
        source_ref: SourceRef,
    },
    MissingDefinition {
        owner: DefId,
    },
    MissingDenseSlot {
        owner: DefId,
        kind: SourceTableSlotKind,
        index: u32,
    },
    DenseIndexOverflow {
        owner: DefId,
        kind: SourceTableSlotKind,
    },
}

impl fmt::Display for SourceTableBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerMismatch {
                table_owner,
                source_ref,
            } => write!(
                formatter,
                "source reference owner {:?} does not match table owner {:?}",
                source_ref.owner(),
                table_owner
            ),
            Self::FileMismatch {
                source_ref,
                expected,
                actual,
            } => write!(
                formatter,
                "source reference {source_ref:?} uses file {actual:?}, expected {expected:?}"
            ),
            Self::RangeOutOfBounds {
                source_ref,
                range,
                source_len,
            } => write!(
                formatter,
                "source reference {source_ref:?} range {range:?} exceeds source length {}",
                source_len.get()
            ),
            Self::DuplicateReference { source_ref } => {
                write!(formatter, "duplicate source reference {source_ref:?}")
            }
            Self::MissingDefinition { owner } => {
                write!(
                    formatter,
                    "source table for {owner:?} has no definition range"
                )
            }
            Self::MissingDenseSlot { owner, kind, index } => write!(
                formatter,
                "source table for {owner:?} is missing {kind:?} slot {index}"
            ),
            Self::DenseIndexOverflow { owner, kind } => write!(
                formatter,
                "source table for {owner:?} has too many {kind:?} slots"
            ),
        }
    }
}

impl std::error::Error for SourceTableBuildError {}

/// A source reference that cannot be resolved by a selected definition table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLookupError {
    OwnerMismatch {
        table_owner: DefId,
        source_ref: SourceRef,
    },
    MissingNode {
        owner: DefId,
        index: u32,
    },
    MissingLocal {
        owner: DefId,
        local: LocalId,
    },
}

impl fmt::Display for SourceLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerMismatch {
                table_owner,
                source_ref,
            } => write!(
                formatter,
                "source reference owner {:?} does not match table owner {:?}",
                source_ref.owner(),
                table_owner
            ),
            Self::MissingNode { owner, index } => {
                write!(
                    formatter,
                    "source table for {owner:?} has no node slot {index}"
                )
            }
            Self::MissingLocal { owner, local } => write!(
                formatter,
                "source table for {owner:?} has no local slot {}",
                local.index()
            ),
        }
    }
}

impl std::error::Error for SourceLookupError {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
