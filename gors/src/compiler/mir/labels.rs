//! Structural label discovery used before MIR control-flow lowering.

use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;

pub(super) fn statement_declares_label(statement: &hir::Stmt) -> bool {
    matches!(
        &statement.kind,
        hir::StmtKind::Label { .. } | hir::StmtKind::For { label: Some(_), .. }
    )
}

pub(super) fn collect_labels(block: &hir::Block, labels: &mut Vec<(String, SourceRef)>) {
    for statement in &block.stmts {
        match &statement.kind {
            hir::StmtKind::Label {
                name,
                statement: body,
            } => {
                labels.push((name.clone(), statement.source));
                if let Some(body) = body {
                    collect_statement_labels(body, labels);
                }
            }
            hir::StmtKind::For { label, .. } => {
                if let Some(label) = label {
                    labels.push((label.clone(), statement.source));
                }
                collect_statement_labels(statement, labels);
            }
            _ => collect_statement_labels(statement, labels),
        }
    }
}

fn collect_statement_labels(statement: &hir::Stmt, labels: &mut Vec<(String, SourceRef)>) {
    match &statement.kind {
        hir::StmtKind::If {
            init,
            then_block,
            else_branch,
            ..
        } => {
            if let Some(init) = init {
                collect_statement_labels(init, labels);
            }
            collect_labels(then_block, labels);
            if let Some(else_branch) = else_branch {
                collect_statement_labels(else_branch, labels);
            }
        }
        hir::StmtKind::For {
            init, post, body, ..
        } => {
            if let Some(init) = init {
                collect_statement_labels(init, labels);
            }
            if let Some(post) = post {
                collect_statement_labels(post, labels);
            }
            collect_labels(body, labels);
        }
        hir::StmtKind::Block(block) => collect_labels(block, labels),
        hir::StmtKind::Label {
            name,
            statement: body,
        } => {
            labels.push((name.clone(), statement.source));
            if let Some(body) = body {
                collect_statement_labels(body, labels);
            }
        }
        hir::StmtKind::Let { .. }
        | hir::StmtKind::LetTuple { .. }
        | hir::StmtKind::Assign { .. }
        | hir::StmtKind::AssignTuple { .. }
        | hir::StmtKind::SliceAssign { .. }
        | hir::StmtKind::Expr(_)
        | hir::StmtKind::Return(_)
        | hir::StmtKind::Goto(_)
        | hir::StmtKind::Break(_)
        | hir::StmtKind::Continue(_) => {}
    }
}
