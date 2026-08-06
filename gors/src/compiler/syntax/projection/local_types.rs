use crate::ast;

use super::{LocalTypeSyntax, ProjectionError, StructuralProjector};

impl StructuralProjector {
    pub(super) fn local_type_spec(
        &mut self,
        spec: &ast::TypeSpec<'_>,
    ) -> Result<LocalTypeSyntax, ProjectionError> {
        let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
        Ok(LocalTypeSyntax {
            name: self.ident(name)?,
            alias: spec.assign.is_some(),
            has_type_parameters: spec.type_params.is_some(),
            target: self.expression(&spec.type_)?,
        })
    }
}
