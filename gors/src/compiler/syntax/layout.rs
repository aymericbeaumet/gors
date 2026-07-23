//! Revision-local physical layout for semantic function syntax.

use crate::source::TextRange;

use super::SyntaxAnchor;

/// Physical byte ranges for one function in the current source revision.
///
/// Layout is presentation/provenance state. It is intentionally separate from
/// semantic token streams and their fingerprints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionLayout {
    anchor: SyntaxAnchor,
    declaration: TextRange,
    header: TextRange,
    body: Option<TextRange>,
}

impl FunctionLayout {
    pub(super) fn new(
        anchor: SyntaxAnchor,
        declaration: TextRange,
        header: TextRange,
        body: Option<TextRange>,
    ) -> Self {
        Self {
            anchor,
            declaration,
            header,
            body,
        }
    }

    /// Stable semantic anchor associated with these revision-local ranges.
    #[must_use]
    pub const fn anchor(&self) -> &SyntaxAnchor {
        &self.anchor
    }

    /// Complete physical declaration range.
    #[must_use]
    pub const fn declaration(&self) -> TextRange {
        self.declaration
    }

    /// Physical range from `func` up to, but excluding, the body brace.
    #[must_use]
    pub const fn header(&self) -> TextRange {
        self.header
    }

    /// Physical braced body range, absent for bodyless declarations.
    #[must_use]
    pub const fn body(&self) -> Option<TextRange> {
        self.body
    }
}
