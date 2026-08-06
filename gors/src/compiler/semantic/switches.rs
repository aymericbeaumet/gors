//! Expression-switch case-body semantics.

use std::sync::Arc;

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, StmtSyntaxKind, SwitchCaseSyntax};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn lower_switch_case_bodies(
        &mut self,
        cases: &[SwitchCaseSyntax],
        redundant_break_label: Option<&str>,
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
                .filter_map(|(index, statement)| match &statement.kind {
                    StmtSyntaxKind::Branch {
                        token: Token::FALLTHROUGH,
                        label,
                    } => Some((index, label)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if fallthrough.len() > 1
                || fallthrough
                    .first()
                    .is_some_and(|(index, label)| Some(*index) != last_statement || label.is_some())
            {
                return Err(Diagnostic::semantic(
                    "fallthrough must be the final non-empty statement of a switch case",
                    source,
                ));
            }
            let fallthrough = fallthrough.first().map(|(index, _)| *index);
            let redundant_break = last_statement.filter(|index| {
                case.body.statements.get(*index).is_some_and(|statement| {
                    matches!(
                        &statement.kind,
                        StmtSyntaxKind::Branch {
                            token: Token::BREAK,
                            label: Some(label),
                        } if redundant_break_label == Some(label.name.as_ref())
                    )
                })
            });
            let block = if let Some(removed) = fallthrough.or(redundant_break) {
                let mut statements = case.body.statements.to_vec();
                statements.remove(removed);
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
