//! Owned semantic syntax and revision-local physical declaration layout.

mod anchor;
mod layout;
mod projection;
mod stream;
#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests;
mod token;

pub use anchor::{SyntaxAnchor, SyntaxAnchorKind};
pub use layout::FunctionLayout;
pub use stream::SemanticTokenStream;
pub use token::SemanticToken;

pub(crate) use projection::{ProjectedFunctionSyntax, project_function};
