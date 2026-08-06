//! Go-statement projection into owned structural syntax.

use crate::ast;

use super::{ProjectionError, StmtSyntaxKind, StructuralProjector};

impl StructuralProjector {
    pub(super) fn go_statement(
        &mut self,
        statement: &ast::GoStmt<'_>,
    ) -> Result<StmtSyntaxKind, ProjectionError> {
        let ast::Expr::FuncLit(function) = statement.call.fun.as_ref() else {
            return Ok(StmtSyntaxKind::Unsupported(
                "go call whose callee is not a function literal",
            ));
        };
        Ok(StmtSyntaxKind::Go {
            has_type_parameters: function.type_.type_params.is_some(),
            params: self.field_list(&function.type_.params)?,
            results: function
                .type_
                .results
                .as_ref()
                .map(|fields| self.field_list(fields))
                .transpose()?,
            body: self.block(&function.body)?,
            arguments: statement
                .call
                .args
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|argument| self.expression(argument))
                .collect::<Result<Vec<_>, _>>()?
                .into(),
            spread: statement.call.ellipsis.is_some(),
        })
    }
}
