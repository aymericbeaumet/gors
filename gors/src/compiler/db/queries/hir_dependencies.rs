//! Dependency traversal over typed HIR.

use std::collections::BTreeSet;

use crate::compiler::hir;
use crate::compiler::ids::QualifiedDefId;

pub(super) fn direct_callees(function: &hir::Function) -> BTreeSet<QualifiedDefId> {
    let mut callees = BTreeSet::new();
    collect_block_callees(&function.body, &mut callees);
    for closure in &function.closures {
        collect_block_callees(&closure.body, &mut callees);
    }
    callees
}

fn collect_block_callees(block: &hir::Block, callees: &mut BTreeSet<QualifiedDefId>) {
    for statement in &block.stmts {
        collect_statement_callees(statement, callees);
    }
}

fn collect_statement_callees(statement: &hir::Stmt, callees: &mut BTreeSet<QualifiedDefId>) {
    match &statement.kind {
        hir::StmtKind::Let { values, .. }
        | hir::StmtKind::Assign { values, .. }
        | hir::StmtKind::Return(values) => {
            for value in values {
                collect_expression_callees(value, callees);
            }
        }
        hir::StmtKind::LetTuple { value, .. } | hir::StmtKind::AssignTuple { value, .. } => {
            collect_expression_callees(value, callees);
        }
        hir::StmtKind::ParallelAssign {
            destinations,
            values,
        } => {
            for destination in destinations {
                match destination {
                    hir::AssignTarget::SliceIndex { slice, index } => {
                        collect_expression_callees(slice, callees);
                        collect_expression_callees(index, callees);
                    }
                    hir::AssignTarget::MapIndex { map, key } => {
                        collect_expression_callees(map, callees);
                        collect_expression_callees(key, callees);
                    }
                    hir::AssignTarget::Local(_) | hir::AssignTarget::Discard => {}
                }
            }
            for value in values {
                collect_expression_callees(value, callees);
            }
        }
        hir::StmtKind::Expr(expression) => collect_expression_callees(expression, callees),
        hir::StmtKind::Defer { values, body, .. } => {
            for value in values {
                collect_expression_callees(value, callees);
            }
            collect_block_callees(body, callees);
        }
        hir::StmtKind::SliceAssign {
            slice,
            index,
            value,
            ..
        } => {
            collect_expression_callees(slice, callees);
            collect_expression_callees(index, callees);
            collect_expression_callees(value, callees);
        }
        hir::StmtKind::ArrayAssign { index, value, .. } => {
            collect_expression_callees(index, callees);
            collect_expression_callees(value, callees);
        }
        hir::StmtKind::MapAssign { map, key, value } => {
            collect_expression_callees(map, callees);
            collect_expression_callees(key, callees);
            collect_expression_callees(value, callees);
        }
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                collect_statement_callees(init, callees);
            }
            collect_expression_callees(condition, callees);
            collect_block_callees(then_block, callees);
            if let Some(branch) = else_branch {
                collect_statement_callees(branch, callees);
            }
        }
        hir::StmtKind::For {
            label: _,
            init,
            condition,
            post,
            body,
        } => {
            if let Some(init) = init {
                collect_statement_callees(init, callees);
            }
            if let Some(condition) = condition {
                collect_expression_callees(condition, callees);
            }
            if let Some(post) = post {
                collect_statement_callees(post, callees);
            }
            collect_block_callees(body, callees);
        }
        hir::StmtKind::Range {
            expression, body, ..
        } => {
            collect_expression_callees(expression, callees);
            collect_block_callees(body, callees);
        }
        hir::StmtKind::Block(block) => collect_block_callees(block, callees),
        hir::StmtKind::Label { statement, .. } => {
            if let Some(statement) = statement {
                collect_statement_callees(statement, callees);
            }
        }
        hir::StmtKind::ClosureBinding(_)
        | hir::StmtKind::Goto(_)
        | hir::StmtKind::Break(_)
        | hir::StmtKind::Continue(_) => {}
    }
}

fn collect_expression_callees(expression: &hir::Expr, callees: &mut BTreeSet<QualifiedDefId>) {
    match &expression.kind {
        hir::ExprKind::Binary { left, right, .. } => {
            collect_expression_callees(left, callees);
            collect_expression_callees(right, callees);
        }
        hir::ExprKind::Unary { operand, .. } => collect_expression_callees(operand, callees),
        hir::ExprKind::Conversion { value } => collect_expression_callees(value, callees),
        hir::ExprKind::Call { callee, args } => {
            if let hir::Callee::Function(definition) = callee {
                callees.insert(*definition);
            }
            for argument in args {
                collect_expression_callees(argument, callees);
            }
        }
        hir::ExprKind::MapLiteralStringI64(entries) => {
            for (key, value) in entries {
                collect_expression_callees(key, callees);
                collect_expression_callees(value, callees);
            }
        }
        hir::ExprKind::ArrayIndexI64 { array, index } => {
            collect_expression_callees(array, callees);
            collect_expression_callees(index, callees);
        }
        hir::ExprKind::ArrayLen { array, .. } => collect_expression_callees(array, callees),
        hir::ExprKind::StructLiteral(fields) => {
            for field in fields {
                collect_expression_callees(field, callees);
            }
        }
        hir::ExprKind::StructField { structure, .. } => {
            collect_expression_callees(structure, callees);
        }
        hir::ExprKind::Constant(_)
        | hir::ExprKind::Local(_)
        | hir::ExprKind::AddressOfLocal(_)
        | hir::ExprKind::GlobalConstant(..)
        | hir::ExprKind::GlobalVariable(..)
        | hir::ExprKind::RecoverCompareNil { .. }
        | hir::ExprKind::SliceLiteralI64(_)
        | hir::ExprKind::SliceLiteralU8(_)
        | hir::ExprKind::ArrayLiteralI64(_) => {}
    }
}
