//! Stable semantic anchors for source declarations.

use std::sync::Arc;

use crate::compiler::fingerprint::{Fingerprint, fingerprint_parts};

/// Semantic declaration category carried by a [`SyntaxAnchor`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyntaxAnchorKind {
    /// A package-level Go function declaration.
    Function,
}

/// A stable declaration anchor independent of layout and traversal order.
///
/// The current frontier supports unique package-level function names, so a
/// function anchor is the declaration kind plus its exact Go name. It never
/// contains a byte offset, token ordinal, or AST traversal index. Repeated
/// `init` declarations remain rejected until a reusable structural
/// disambiguator exists.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyntaxAnchor {
    kind: SyntaxAnchorKind,
    name: Arc<str>,
    fingerprint: Fingerprint,
}

impl SyntaxAnchor {
    /// Construct the stable anchor for one uniquely named package function.
    #[must_use]
    pub fn named_function(name: impl Into<Arc<str>>) -> Self {
        let name = name.into();
        let fingerprint = fingerprint_parts(b"syntax-anchor-function-v1", &[name.as_bytes()]);
        Self {
            kind: SyntaxAnchorKind::Function,
            name,
            fingerprint,
        }
    }

    /// Declaration category.
    #[must_use]
    pub const fn kind(&self) -> SyntaxAnchorKind {
        self.kind
    }

    /// Stable semantic name used to disambiguate this declaration.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Domain-separated semantic anchor fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}
