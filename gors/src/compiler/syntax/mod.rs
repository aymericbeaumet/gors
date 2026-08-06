//! Owned semantic syntax and revision-local physical declaration layout.

mod anchor;
mod layout;
mod projection;
mod stream;
mod structural;
#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests;
mod token;

pub use anchor::{SyntaxAnchor, SyntaxAnchorKind};
pub use layout::{ConstantLayout, FunctionLayout};
pub use stream::SemanticTokenStream;
pub use structural::{
    BlockSyntax, ChannelDirectionSyntax, ConstantSyntax, ConstantValueSyntax, DeclSyntax,
    ExprSyntax, ExprSyntaxKind, FieldListSyntax, FieldSyntax, FunctionBodySyntax,
    FunctionHeaderSyntax, IdentSyntax, StmtSyntax, StmtSyntaxKind, SwitchCaseSyntax, SyntaxSource,
    SyntaxSourceRegion, TypeAliasSyntax, TypeDefinitionSyntax, ValueSpecSyntax,
};
pub use token::SemanticToken;

pub(crate) use projection::{
    ProjectedConstantSyntax, ProjectedFunctionSyntax, project_constant, project_function,
    project_type_alias, project_type_definition,
};
