//! Expression-switch case-body semantics.

use std::sync::Arc;

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, StmtSyntax, StmtSyntaxKind, SwitchCaseSyntax};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn lower_switch_case_bodies(
        &mut self,
        cases: &[SwitchCaseSyntax],
        source: SourceRef,
    ) -> Result<Vec<hir::Block>, Diagnostic> {
        let mut bodies = Vec::with_capacity(cases.len());
        let mut falls_through = Vec::with_capacity(cases.len());
        for case in cases {
            let last_statement = case
                .body
                .statements
                .iter()
                .rposition(|statement| !matches!(statement.kind, StmtSyntaxKind::Empty));
            let fallthrough = case
                .body
                .statements
                .iter()
                .enumerate()
                .filter_map(|(index, statement)| switch_fallthrough(statement).then_some(index))
                .collect::<Vec<_>>();
            if fallthrough.len() > 1
                || fallthrough
                    .first()
                    .is_some_and(|index| Some(*index) != last_statement)
            {
                return Err(Diagnostic::semantic(
                    "fallthrough must be the final non-empty statement of a switch case",
                    source,
                ));
            }
            let fallthrough = fallthrough.first().copied();
            let block = if let Some(removed) = fallthrough {
                let mut statements = case.body.statements.to_vec();
                let statement = statements.remove(removed);
                if let Some(labels) = strip_switch_fallthrough(statement) {
                    statements.insert(removed, labels);
                }
                BlockSyntax {
                    source: case.body.source,
                    statements: Arc::from(statements),
                }
            } else {
                case.body.clone()
            };
            bodies.push(self.lower_block(&block, true)?);
            falls_through.push(fallthrough.is_some());
        }

        for index in (0..bodies.len()).rev() {
            if !falls_through.get(index).copied().unwrap_or(false) {
                continue;
            }
            let Some(next) = bodies.get(index + 1).cloned() else {
                return Err(Diagnostic::semantic(
                    "the final switch case cannot fall through",
                    source,
                ));
            };
            let case_source = cases
                .get(index)
                .ok_or_else(|| Diagnostic::backend("switch case body count changed"))?
                .source;
            let node = self.alloc_node(case_source)?;
            let body = bodies
                .get_mut(index)
                .ok_or_else(|| Diagnostic::backend("switch case body disappeared"))?;
            body.stmts.push(hir::Stmt {
                node,
                kind: hir::StmtKind::Block(next),
                source: SourceRef::node(node),
            });
        }
        Ok(bodies)
    }
}

fn switch_fallthrough(statement: &StmtSyntax) -> bool {
    match &statement.kind {
        StmtSyntaxKind::Branch {
            token: Token::FALLTHROUGH,
            label: None,
        } => true,
        StmtSyntaxKind::Labeled { statement, .. } => switch_fallthrough(statement),
        _ => false,
    }
}

/// Remove the fallthrough leaf while preserving every label that targeted it.
/// The empty leaf lowers to `hir::StmtKind::Label { statement: None }`, so a
/// preceding goto still reaches the exact source position before control
/// continues into the next switch clause.
fn strip_switch_fallthrough(statement: StmtSyntax) -> Option<StmtSyntax> {
    let StmtSyntax { source, kind } = statement;
    match kind {
        StmtSyntaxKind::Branch {
            token: Token::FALLTHROUGH,
            label: None,
        } => None,
        StmtSyntaxKind::Labeled { label, statement } => {
            let nested_source = statement.source;
            let statement = strip_switch_fallthrough(*statement).unwrap_or(StmtSyntax {
                source: nested_source,
                kind: StmtSyntaxKind::Empty,
            });
            Some(StmtSyntax {
                source,
                kind: StmtSyntaxKind::Labeled {
                    label,
                    statement: Box::new(statement),
                },
            })
        }
        kind => Some(StmtSyntax { source, kind }),
    }
}
