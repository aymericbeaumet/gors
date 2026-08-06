//! Typed recovery-value patterns shared by expression and statement lowering.

use super::FunctionLowerer;
use super::expressions::coerce_expr;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, StmtSyntax, StmtSyntaxKind,
};
use crate::compiler::types::{ConstValue, Ty};
use crate::token::Token;

pub(super) struct RecoverIfComparison<'a> {
    pub(super) arguments: &'a [ExprSyntax],
    pub(super) spread: bool,
    pub(super) equal: bool,
}

impl FunctionLowerer {
    pub(super) fn lower_if_statement(
        &mut self,
        init: Option<&StmtSyntax>,
        condition: &ExprSyntax,
        then_block: &BlockSyntax,
        else_branch: Option<&StmtSyntax>,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        self.push_scope();
        let recover = init.and_then(|init| recover_if_comparison(init, condition));
        let (init, condition) = if let Some(recover) = recover {
            if recover.spread || !recover.arguments.is_empty() {
                return Err(Diagnostic::semantic(
                    "recover requires no arguments",
                    source,
                ));
            }
            let condition_node = self.alloc_node(condition.source)?;
            let condition_source = SourceRef::node(condition_node);
            (
                None,
                self.lower_recover_nil_comparison(
                    condition_node,
                    condition_source,
                    recover.equal,
                    Some(&Ty::Bool),
                )?,
            )
        } else {
            (
                init.map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new),
                self.lower_expr(condition, Some(&Ty::Bool))?,
            )
        };
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

    pub(super) fn lower_recover_nil_comparison(
        &mut self,
        node: NodeId,
        source: SourceRef,
        equal: bool,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let mut recovered = if self.inside_deferred_closure {
            hir::Expr {
                node,
                kind: hir::ExprKind::RecoverCompareNil { equal },
                ty: Ty::Bool,
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_read: true,
                    may_write: true,
                    ..hir::Effects::default()
                },
                source,
            }
        } else {
            hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Bool(equal)),
                ty: Ty::Bool,
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut recovered, expected, source)?;
        }
        Ok(recovered)
    }
}

pub(super) fn recover_nil_comparison<'a>(
    left: &'a ExprSyntax,
    right: &'a ExprSyntax,
) -> Option<(&'a [ExprSyntax], bool)> {
    recover_call(left)
        .filter(|_| is_nil_identifier(right))
        .or_else(|| recover_call(right).filter(|_| is_nil_identifier(left)))
}

pub(super) fn recover_if_comparison<'a>(
    init: &'a StmtSyntax,
    condition: &ExprSyntax,
) -> Option<RecoverIfComparison<'a>> {
    let StmtSyntaxKind::Assign { left, token, right } = &init.kind else {
        return None;
    };
    let ([binding], Token::DEFINE, [value]) = (left.as_ref(), *token, right.as_ref()) else {
        return None;
    };
    let ExprSyntaxKind::Ident(binding) = &binding.kind else {
        return None;
    };
    let (arguments, spread) = recover_call(value)?;
    let ExprSyntaxKind::Binary { left, token, right } = &condition.kind else {
        return None;
    };
    if !matches!(token, Token::EQL | Token::NEQ) {
        return None;
    }
    let compares_binding_to_nil = is_ident(left, &binding.name) && is_nil_identifier(right)
        || is_nil_identifier(left) && is_ident(right, &binding.name);
    compares_binding_to_nil.then_some(RecoverIfComparison {
        arguments,
        spread,
        equal: *token == Token::EQL,
    })
}

fn recover_call(expression: &ExprSyntax) -> Option<(&[ExprSyntax], bool)> {
    let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread,
    } = &expression.kind
    else {
        return None;
    };
    let ExprSyntaxKind::Ident(callee) = &callee.kind else {
        return None;
    };
    (callee.name.as_ref() == "recover").then_some((arguments, *spread))
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    is_ident(expression, "nil")
}

fn is_ident(expression: &ExprSyntax, expected: &str) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == expected
    )
}
