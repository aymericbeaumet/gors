//! Package type-declaration projection.

use super::{ProjectionError, StructuralProjector};
use crate::ast;
use crate::compiler::syntax::{SyntaxSourceRegion, TypeAliasSyntax, TypeDefinitionSyntax};

pub fn project_type_alias(spec: &ast::TypeSpec<'_>) -> Result<TypeAliasSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::Constant);
    Ok(TypeAliasSyntax {
        name: projector.ident(name)?,
        type_parameters: spec
            .type_params
            .as_ref()
            .map(|parameters| projector.field_list(parameters))
            .transpose()?,
        target: projector.expression(&spec.type_)?,
    })
}

pub fn project_type_definition(
    spec: &ast::TypeSpec<'_>,
) -> Result<TypeDefinitionSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::Constant);
    Ok(TypeDefinitionSyntax {
        name: projector.ident(name)?,
        type_parameters: spec
            .type_params
            .as_ref()
            .map(|parameters| projector.field_list(parameters))
            .transpose()?,
        underlying: projector.expression(&spec.type_)?,
    })
}
