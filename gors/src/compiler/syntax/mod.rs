//! Owned semantic syntax and revision-local physical declaration layout.

mod anchor;
mod layout;
mod projected;
mod projection;
mod stream;
mod structural;
#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests;
mod token;

pub use anchor::{SyntaxAnchor, SyntaxAnchorKind};
pub use layout::{ConstantLayout, FunctionLayout, VariableLayout};
pub use stream::SemanticTokenStream;
pub use structural::{
    BlockSyntax, ChannelDirectionSyntax, ConstantSyntax, ConstantValueSyntax, DeclSyntax,
    ExprSyntax, ExprSyntaxKind, FieldListSyntax, FieldSyntax, FunctionBodySyntax,
    FunctionHeaderSyntax, IdentSyntax, LocalTypeSyntax, SelectCaseSyntax, StmtSyntax,
    StmtSyntaxKind, SwitchCaseSyntax, SyntaxSource, SyntaxSourceRegion, TypeAliasSyntax,
    TypeDefinitionSyntax, ValueSpecSyntax, VariableSyntax, VariableValueSyntax,
};
pub use token::SemanticToken;

pub(crate) use projected::{
    ProjectedConstantSyntax, ProjectedFunctionSyntax, ProjectedVariableSyntax,
};
pub(crate) use projection::{
    project_constant, project_function, project_type_alias, project_type_definition,
    project_variable,
};
pub(crate) use structural::{function_is_generic, method_receiver};
