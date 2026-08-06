//! Dataflow proofs for per-iteration value captures.

use std::collections::BTreeSet;

use crate::compiler::syntax::{BlockSyntax, ExprSyntaxKind, StmtSyntax, StmtSyntaxKind};
use crate::token::Token;

pub(super) fn assigned_names_in_block(block: &BlockSyntax) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for statement in &*block.statements {
        collect_assigned_names(statement, &mut names);
    }
    names
}

fn collect_assigned_names(statement: &StmtSyntax, names: &mut BTreeSet<String>) {
    match &statement.kind {
        StmtSyntaxKind::Assign { left, .. } => {
            for target in &**left {
                if let ExprSyntaxKind::Ident(ident) = &target.kind {
                    names.insert(ident.name.to_string());
                }
            }
        }
        StmtSyntaxKind::IncDec { expression, .. } => {
            if let ExprSyntaxKind::Ident(ident) = &expression.kind {
                names.insert(ident.name.to_string());
            }
        }
        StmtSyntaxKind::Block(block) => names.extend(assigned_names_in_block(block)),
        StmtSyntaxKind::If {
            init,
            then_block,
            else_branch,
            ..
        } => {
            if let Some(init) = init {
                collect_assigned_names(init, names);
            }
            names.extend(assigned_names_in_block(then_block));
            if let Some(branch) = else_branch {
                collect_assigned_names(branch, names);
            }
        }
        StmtSyntaxKind::For {
            init, post, body, ..
        } => {
            if let Some(init) = init {
                collect_assigned_names(init, names);
            }
            names.extend(assigned_names_in_block(body));
            if let Some(post) = post {
                collect_assigned_names(post, names);
            }
        }
        StmtSyntaxKind::Range {
            key,
            value,
            token,
            body,
            ..
        } => {
            if *token == Some(Token::ASSIGN) {
                for target in [key.as_ref(), value.as_ref()].into_iter().flatten() {
                    if let ExprSyntaxKind::Ident(ident) = &target.kind {
                        names.insert(ident.name.to_string());
                    }
                }
            }
            names.extend(assigned_names_in_block(body));
        }
        StmtSyntaxKind::Switch { init, cases, .. }
        | StmtSyntaxKind::TypeSwitch { init, cases, .. } => {
            if let Some(init) = init {
                collect_assigned_names(init, names);
            }
            for case in &**cases {
                names.extend(assigned_names_in_block(&case.body));
            }
        }
        StmtSyntaxKind::Select { cases } => {
            for case in &**cases {
                if let Some(communication) = &case.communication {
                    collect_assigned_names(communication, names);
                }
                names.extend(assigned_names_in_block(&case.body));
            }
        }
        StmtSyntaxKind::Labeled { statement, .. } => collect_assigned_names(statement, names),
        StmtSyntaxKind::Defer { body, .. } | StmtSyntaxKind::Go { body, .. } => {
            names.extend(assigned_names_in_block(body));
        }
        StmtSyntaxKind::Empty
        | StmtSyntaxKind::Expr(_)
        | StmtSyntaxKind::Decl(_)
        | StmtSyntaxKind::Send { .. }
        | StmtSyntaxKind::Return(_)
        | StmtSyntaxKind::Branch { .. }
        | StmtSyntaxKind::Unsupported(_) => {}
    }
}
