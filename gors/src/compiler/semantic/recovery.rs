//! Typed panic recovery semantics.

use super::FunctionLowerer;
use super::expressions::coerce_expr;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, ExprSyntax, StmtSyntax};
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_if_statement(
        &mut self,
        init: Option<&StmtSyntax>,
        condition: &ExprSyntax,
        then_block: &BlockSyntax,
        else_branch: Option<&StmtSyntax>,
        _source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        self.push_scope();
        let init = init
            .map(|statement| self.lower_stmt(statement))
            .transpose()?
            .flatten()
            .map(Box::new);
        let condition = self.lower_expr(condition, Some(&Ty::Bool))?;
        let then_block = self.lower_block(then_block, true)?;
        let else_branch = else_branch
            .map(|statement| self.lower_stmt(statement))
            .transpose()?
            .flatten()
            .map(Box::new);
        self.pop_scope();
        Ok(hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        })
    }

    pub(super) fn lower_recover_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread || !arguments.is_empty() {
            return Err(Diagnostic::semantic(
                "recover requires no arguments",
                source,
            ));
        }

        let ty = Ty::Interface(Vec::new());
        let mut recovered = if self.inside_deferred_closure && !self.inside_local_closure {
            hir::Expr {
                node,
                kind: hir::ExprKind::Recover,
                ty,
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_read: true,
                    may_write: true,
                    ..hir::Effects::default()
                },
                source,
            }
        } else {
            self.zero_value_expr(node, source, ty)?
        };
        if let Some(expected) = expected {
            coerce_expr(&mut recovered, expected, source)?;
        }
        Ok(recovered)
    }
}
