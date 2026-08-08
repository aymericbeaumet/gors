//! Lexical resolution of break and continue to exact semantic targets.

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::ids::ControlTargetId;
use crate::compiler::provenance::SourceRef;
use crate::token::Token;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ControlTargetKind {
    BreakOnly,
    Loop,
}

#[derive(Clone, Debug)]
pub(super) struct ActiveControlTarget {
    pub(super) id: ControlTargetId,
    pub(super) kind: ControlTargetKind,
    pub(super) label: Option<String>,
}

impl FunctionLowerer {
    pub(super) fn begin_control_target(
        &mut self,
        kind: ControlTargetKind,
        label: Option<String>,
    ) -> Result<ControlTargetId, Diagnostic> {
        let id = ControlTargetId(self.next_control_target);
        self.next_control_target = self
            .next_control_target
            .checked_add(1)
            .ok_or_else(|| Diagnostic::backend("function exceeds the control target ID space"))?;
        self.control_targets
            .push(ActiveControlTarget { id, kind, label });
        Ok(id)
    }

    pub(super) fn end_control_target(
        &mut self,
        expected: ControlTargetId,
    ) -> Result<(), Diagnostic> {
        let target = self
            .control_targets
            .pop()
            .ok_or_else(|| Diagnostic::backend("semantic control target stack underflow"))?;
        if target.id != expected {
            return Err(Diagnostic::backend(
                "semantic control targets were not closed in lexical order",
            ));
        }
        Ok(())
    }

    pub(super) fn resolve_control_target(
        &self,
        token: Token,
        label: Option<&str>,
        source: SourceRef,
    ) -> Result<ControlTargetId, Diagnostic> {
        let target = match (token, label) {
            (Token::BREAK, Some(label)) => self
                .control_targets
                .iter()
                .rev()
                .find(|target| target.label.as_deref() == Some(label)),
            (Token::BREAK, None) => self.control_targets.last(),
            (Token::CONTINUE, Some(label)) => {
                let target = self
                    .control_targets
                    .iter()
                    .rev()
                    .find(|target| target.label.as_deref() == Some(label))
                    .ok_or_else(|| {
                        Diagnostic::semantic(
                            format!(
                                "continue label {label} does not denote an enclosing statement"
                            ),
                            source,
                        )
                    })?;
                if target.kind != ControlTargetKind::Loop {
                    return Err(Diagnostic::semantic(
                        format!("continue label {label} does not denote a for loop"),
                        source,
                    ));
                }
                return Ok(target.id);
            }
            (Token::CONTINUE, None) => self
                .control_targets
                .iter()
                .rev()
                .find(|target| target.kind == ControlTargetKind::Loop),
            _ => None,
        };
        target.map(|target| target.id).ok_or_else(|| {
            let message = match (token, label) {
                (Token::BREAK, Some(label)) => {
                    format!(
                        "break label {label} does not denote an enclosing for, switch, or select"
                    )
                }
                (Token::BREAK, None) => {
                    "break does not target an enclosing for, switch, or select".to_owned()
                }
                (Token::CONTINUE, None) => {
                    "continue does not target an enclosing for loop".to_owned()
                }
                _ => "branch does not target a supported enclosing statement".to_owned(),
            };
            Diagnostic::semantic(message, source)
        })
    }
}
