//! Separation of stable function semantics from revision-local file offsets.

use std::sync::Arc;

use crate::compiler::hir;
use crate::compiler::ids::SourceSpan;

use super::products::FunctionProvenance;

pub(super) fn make_function_relative(function: &mut hir::Function) -> FunctionProvenance {
    let anchor = function.span.clone();
    let provenance = FunctionProvenance::new(
        Arc::from(anchor.file.as_str()),
        anchor.start,
        anchor.line,
        anchor.column,
    );
    let base = RelativeBase {
        offset: anchor.start,
        line: anchor.line,
        column: anchor.column,
    };
    relative_span(&mut function.span, base);
    for local in &mut function.locals {
        relative_span(&mut local.span, base);
    }
    relative_block(&mut function.body, base);
    provenance
}

#[derive(Clone, Copy)]
struct RelativeBase {
    offset: usize,
    line: usize,
    column: usize,
}

fn relative_block(block: &mut hir::Block, base: RelativeBase) {
    relative_span(&mut block.span, base);
    for statement in &mut block.stmts {
        relative_statement(statement, base);
    }
}

fn relative_statement(statement: &mut hir::Stmt, base: RelativeBase) {
    relative_span(&mut statement.span, base);
    match &mut statement.kind {
        hir::StmtKind::Let { values, .. } | hir::StmtKind::Assign { values, .. } => {
            for value in values {
                relative_expression(value, base);
            }
        }
        hir::StmtKind::Expr(expression) => relative_expression(expression, base),
        hir::StmtKind::Return(values) => {
            for value in values {
                relative_expression(value, base);
            }
        }
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                relative_statement(init, base);
            }
            relative_expression(condition, base);
            relative_block(then_block, base);
            if let Some(branch) = else_branch {
                relative_statement(branch, base);
            }
        }
        hir::StmtKind::For {
            init,
            condition,
            post,
            body,
        } => {
            if let Some(init) = init {
                relative_statement(init, base);
            }
            if let Some(condition) = condition {
                relative_expression(condition, base);
            }
            if let Some(post) = post {
                relative_statement(post, base);
            }
            relative_block(body, base);
        }
        hir::StmtKind::Block(block) => relative_block(block, base),
        hir::StmtKind::Break | hir::StmtKind::Continue => {}
    }
}

fn relative_expression(expression: &mut hir::Expr, base: RelativeBase) {
    relative_span(&mut expression.span, base);
    match &mut expression.kind {
        hir::ExprKind::Binary { left, right, .. } => {
            relative_expression(left, base);
            relative_expression(right, base);
        }
        hir::ExprKind::Unary { operand, .. } => relative_expression(operand, base),
        hir::ExprKind::Call { args, .. } => {
            for argument in args {
                relative_expression(argument, base);
            }
        }
        hir::ExprKind::Constant(_)
        | hir::ExprKind::Local(_)
        | hir::ExprKind::GlobalConstant(..) => {}
    }
}

fn relative_span(span: &mut SourceSpan, base: RelativeBase) {
    if span.file.is_empty() || span.line == 0 || span.column == 0 {
        return;
    }
    span.start = span.start.saturating_sub(base.offset);
    span.end = span.end.saturating_sub(base.offset);
    if span.line == base.line {
        span.column = span.column.saturating_sub(base.column).saturating_add(1);
    }
    span.line = span.line.saturating_sub(base.line).saturating_add(1);
}
