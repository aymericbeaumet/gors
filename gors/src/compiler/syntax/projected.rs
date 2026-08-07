//! Owned products from one parser-to-semantic-syntax projection.

use super::{
    ConstantLayout, ConstantSyntax, FunctionBodySyntax, FunctionHeaderSyntax, FunctionLayout,
    SemanticTokenStream, SyntaxAnchor, TypeAliasSyntax, TypeDeclarationLayout,
    TypeDefinitionSyntax, VariableLayout, VariableSyntax,
};

pub struct ProjectedFunctionSyntax {
    pub(crate) anchor: SyntaxAnchor,
    pub(crate) layout: FunctionLayout,
    pub(crate) header: SemanticTokenStream,
    pub(crate) body: Option<SemanticTokenStream>,
    pub(crate) structural_header: FunctionHeaderSyntax,
    pub(crate) structural_body: FunctionBodySyntax,
}

pub struct ProjectedConstantSyntax {
    pub(crate) syntax: ConstantSyntax,
    pub(crate) layout: ConstantLayout,
}

pub struct ProjectedVariableSyntax {
    pub(crate) syntax: VariableSyntax,
    pub(crate) layout: VariableLayout,
}

pub struct ProjectedTypeAliasSyntax {
    pub(crate) syntax: TypeAliasSyntax,
    pub(crate) layout: TypeDeclarationLayout,
}

pub struct ProjectedTypeDefinitionSyntax {
    pub(crate) syntax: TypeDefinitionSyntax,
    pub(crate) layout: TypeDeclarationLayout,
}
