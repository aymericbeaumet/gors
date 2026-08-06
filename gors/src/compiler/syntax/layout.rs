//! Revision-local physical layout for semantic function syntax.

use std::fmt;
use std::sync::Arc;

use crate::source::{TextRange, TextSize};

use super::{SyntaxAnchor, SyntaxSource, SyntaxSourceRegion};

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
    source_len: TextSize,
    header_sources: Arc<[TextRange]>,
    body_sources: Arc<[TextRange]>,
}

impl FunctionLayout {
    pub(super) fn new(
        anchor: SyntaxAnchor,
        declaration: TextRange,
        header: TextRange,
        body: Option<TextRange>,
        source_len: TextSize,
        header_sources: Arc<[TextRange]>,
        body_sources: Arc<[TextRange]>,
    ) -> Self {
        Self {
            anchor,
            declaration,
            header,
            body,
            source_len,
            header_sources,
            body_sources,
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

    #[must_use]
    pub const fn source_len(&self) -> TextSize {
        self.source_len
    }

    /// Resolve one owned structural source identity in this revision.
    pub(crate) fn resolve(&self, source: SyntaxSource) -> Result<TextRange, SyntaxLayoutError> {
        let ranges = match source.region() {
            SyntaxSourceRegion::Header => &self.header_sources,
            SyntaxSourceRegion::Body => &self.body_sources,
            SyntaxSourceRegion::Constant | SyntaxSourceRegion::Variable => {
                return Err(SyntaxLayoutError::WrongRegion {
                    expected: "function header or body",
                    actual: source.region(),
                });
            }
        };
        usize::try_from(source.index())
            .ok()
            .and_then(|index| ranges.get(index))
            .copied()
            .ok_or(SyntaxLayoutError::MissingSource(source))
    }
}

/// Revision-local physical source layout for one package constant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstantLayout {
    declaration: TextRange,
    source_len: TextSize,
    sources: Arc<[TextRange]>,
}

impl ConstantLayout {
    pub(super) fn new(
        declaration: TextRange,
        source_len: TextSize,
        sources: Arc<[TextRange]>,
    ) -> Self {
        Self {
            declaration,
            source_len,
            sources,
        }
    }

    #[must_use]
    pub const fn declaration(&self) -> TextRange {
        self.declaration
    }

    #[must_use]
    pub const fn source_len(&self) -> TextSize {
        self.source_len
    }
}

/// Revision-local physical source layout for one package variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariableLayout {
    declaration: TextRange,
    source_len: TextSize,
    sources: Arc<[TextRange]>,
}

impl VariableLayout {
    pub(super) fn new(
        declaration: TextRange,
        source_len: TextSize,
        sources: Arc<[TextRange]>,
    ) -> Self {
        Self {
            declaration,
            source_len,
            sources,
        }
    }

    #[must_use]
    pub const fn declaration(&self) -> TextRange {
        self.declaration
    }

    #[must_use]
    pub const fn source_len(&self) -> TextSize {
        self.source_len
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxLayoutError {
    MissingSource(SyntaxSource),
    WrongRegion {
        expected: &'static str,
        actual: SyntaxSourceRegion,
    },
}

impl fmt::Display for SyntaxLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSource(source) => write!(
                formatter,
                "structural source {:?}:{} is absent from the physical layout",
                source.region(),
                source.index()
            ),
            Self::WrongRegion { expected, actual } => {
                write!(formatter, "expected {expected} source, found {actual:?}")
            }
        }
    }
}
