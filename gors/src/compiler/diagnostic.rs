//! Stage-neutral compiler diagnostics.

use std::fmt;

use super::ids::SourceSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: SourceSpan,
}

impl Diagnostic {
    pub(super) fn unsupported(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            code: "GORS2001",
            message: message.into(),
            span,
        }
    }

    pub(super) fn semantic(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            code: "GORS2002",
            message: message.into(),
            span,
        }
    }

    pub(super) fn backend(message: impl Into<String>) -> Self {
        Self {
            code: "GORS2003",
            message: message.into(),
            span: SourceSpan::synthetic(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.span.file.is_empty() {
            write!(f, "{}: {}", self.code, self.message)
        } else {
            write!(
                f,
                "{}:{}:{}: {}: {}",
                self.span.file, self.span.line, self.span.column, self.code, self.message
            )
        }
    }
}
