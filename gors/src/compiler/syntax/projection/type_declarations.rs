//! Package type-declaration projection.

use super::{ProjectionError, StructuralProjector};
use crate::ast;
use crate::compiler::syntax::{
    ProjectedTypeAliasSyntax, ProjectedTypeDefinitionSyntax, SyntaxSourceRegion, TypeAliasSyntax,
    TypeDeclarationLayout, TypeDefinitionSyntax,
};
use crate::source::TextSize;

pub fn project_type_alias(
    spec: &ast::TypeSpec<'_>,
    source_len: TextSize,
) -> Result<ProjectedTypeAliasSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::TypeDeclaration);
    let name = projector.ident(name)?;
    let declaration = projector.range(name.source)?;
    let syntax = TypeAliasSyntax {
        name,
        type_parameters: spec
            .type_params
            .as_ref()
            .map(|parameters| projector.field_list(parameters))
            .transpose()?,
        target: projector.expression(&spec.type_)?,
    };
    Ok(ProjectedTypeAliasSyntax {
        syntax,
        layout: TypeDeclarationLayout::new(declaration, source_len, projector.finish()),
    })
}

pub fn project_type_definition(
    spec: &ast::TypeSpec<'_>,
    source_len: TextSize,
) -> Result<ProjectedTypeDefinitionSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::TypeDeclaration);
    let name = projector.ident(name)?;
    let declaration = projector.range(name.source)?;
    let syntax = TypeDefinitionSyntax {
        name,
        type_parameters: spec
            .type_params
            .as_ref()
            .map(|parameters| projector.field_list(parameters))
            .transpose()?,
        underlying: projector.expression(&spec.type_)?,
    };
    Ok(ProjectedTypeDefinitionSyntax {
        syntax,
        layout: TypeDeclarationLayout::new(declaration, source_len, projector.finish()),
    })
}
