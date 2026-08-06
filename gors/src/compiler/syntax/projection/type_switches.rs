//! Type-switch projection into owned structural syntax.

use std::sync::Arc;

use crate::ast;
use crate::token::Token;

use super::{BlockSyntax, ProjectionError, StmtSyntaxKind, StructuralProjector, SwitchCaseSyntax};

impl StructuralProjector {
    pub(super) fn type_switch_statement(
        &mut self,
        statement: &ast::TypeSwitchStmt<'_>,
    ) -> Result<StmtSyntaxKind, ProjectionError> {
        let init = statement
            .init
            .as_deref()
            .map(|statement| self.statement(statement).map(Box::new))
            .transpose()?;
        let (binding, expression) = match statement.assign.as_ref() {
            ast::Stmt::ExprStmt(guard) => (None, type_switch_expression(&guard.x)?),
            ast::Stmt::AssignStmt(guard) if guard.tok == Token::DEFINE => {
                let ([left], [right]) = (guard.lhs.as_slice(), guard.rhs.as_slice()) else {
                    return Err(ProjectionError::InvalidTypeSwitchGuard);
                };
                let ast::Expr::Ident(binding) = left else {
                    return Err(ProjectionError::InvalidTypeSwitchGuard);
                };
                (Some(self.ident(binding)?), type_switch_expression(right)?)
            }
            _ => return Err(ProjectionError::InvalidTypeSwitchGuard),
        };
        let expression = self.expression(expression)?;

        let mut cases = Vec::new();
        for statement in &statement.body.list {
            let ast::Stmt::CaseClause(case) = statement else {
                return Err(ProjectionError::InvalidSwitchBody);
            };
            let source = self.source(&case.case)?;
            let expressions = case
                .list
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|expression| self.expression(expression))
                .collect::<Result<Vec<_>, _>>()?;
            let body_source = self.source(&case.colon)?;
            let body = case
                .body
                .iter()
                .map(|statement| self.statement(statement))
                .collect::<Result<Vec<_>, _>>()?;
            cases.push(SwitchCaseSyntax {
                source,
                expressions: Arc::from(expressions),
                body: BlockSyntax {
                    source: body_source,
                    statements: Arc::from(body),
                },
            });
        }

        Ok(StmtSyntaxKind::TypeSwitch {
            init,
            binding,
            expression,
            cases: Arc::from(cases),
        })
    }
}

fn type_switch_expression<'ast, 'source>(
    expression: &'ast ast::Expr<'source>,
) -> Result<&'ast ast::Expr<'source>, ProjectionError> {
    let ast::Expr::TypeAssertExpr(assertion) = expression else {
        return Err(ProjectionError::InvalidTypeSwitchGuard);
    };
    if assertion.type_.is_some() {
        return Err(ProjectionError::InvalidTypeSwitchGuard);
    }
    Ok(&assertion.x)
}
