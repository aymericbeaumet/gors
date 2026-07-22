//! Stage-neutral compiler diagnostics.

use std::fmt;

use super::provenance::{FileRange, SourceRef};

/// Source ownership for a diagnostic before presentation coordinates exist.
///
/// Frontend diagnostics carry an exact physical range. Diagnostics produced
/// from retained semantic artifacts carry an owner-local source reference
/// which the session resolves through the current definition source table.
/// Checkout paths and Go `//line` projections deliberately live outside this
/// stage product.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticLocation {
    Synthetic,
    Physical(FileRange),
    Source(SourceRef),
}

impl From<FileRange> for DiagnosticLocation {
    fn from(range: FileRange) -> Self {
        Self::Physical(range)
    }
}

impl From<SourceRef> for DiagnosticLocation {
    fn from(source: SourceRef) -> Self {
        Self::Source(source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub location: DiagnosticLocation,
}

impl Diagnostic {
    pub(super) fn unsupported(
        message: impl Into<String>,
        location: impl Into<DiagnosticLocation>,
    ) -> Self {
        Self {
            code: "GORS2001",
            message: message.into(),
            location: location.into(),
        }
    }

    pub(super) fn semantic(
        message: impl Into<String>,
        location: impl Into<DiagnosticLocation>,
    ) -> Self {
        Self {
            code: "GORS2002",
            message: message.into(),
            location: location.into(),
        }
    }

    pub(super) fn backend(message: impl Into<String>) -> Self {
        Self {
            code: "GORS2003",
            message: message.into(),
            location: DiagnosticLocation::Synthetic,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
