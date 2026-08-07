//! Exact semantic-control-target mapping during HIR-to-MIR lowering.

use super::super::construct::make_terminator;
use super::super::{Provenance, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, ControlTargetId};
use crate::compiler::provenance::SourceRef;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlTargetKind {
    BreakOnly,
    Loop { continue_target: BasicBlockId },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ControlTargets {
    kind: ControlTargetKind,
    break_target: BasicBlockId,
    pub(super) break_used: bool,
}

impl FunctionLowerer {
    pub(super) fn enter_loop_target(
        &mut self,
        id: ControlTargetId,
        break_target: BasicBlockId,
        continue_target: BasicBlockId,
    ) -> Result<(), Diagnostic> {
        self.enter_control_target(
            id,
            ControlTargets {
                kind: ControlTargetKind::Loop { continue_target },
                break_target,
                break_used: false,
            },
        )
    }

    fn enter_break_target(
        &mut self,
        id: ControlTargetId,
        break_target: BasicBlockId,
    ) -> Result<(), Diagnostic> {
        self.enter_control_target(
            id,
            ControlTargets {
                kind: ControlTargetKind::BreakOnly,
                break_target,
                break_used: false,
            },
        )
    }

    fn enter_control_target(
        &mut self,
        id: ControlTargetId,
        targets: ControlTargets,
    ) -> Result<(), Diagnostic> {
        if self.control_targets.iter().any(|(active, _)| *active == id) {
            return Err(Diagnostic::backend(format!(
                "duplicate active HIR control target {}",
                id.0
            )));
        }
        self.control_targets.push((id, targets));
        Ok(())
    }

    pub(super) fn exit_control_target(
        &mut self,
        expected: ControlTargetId,
    ) -> Result<ControlTargets, Diagnostic> {
        let (id, targets) = self
            .control_targets
            .pop()
            .ok_or_else(|| Diagnostic::backend("MIR control target stack underflow"))?;
        if id != expected {
            return Err(Diagnostic::backend(format!(
                "HIR control target {} closed while target {} was active",
                expected.0, id.0
            )));
        }
        Ok(targets)
    }

    pub(super) fn break_block(&mut self, id: ControlTargetId) -> Result<BasicBlockId, Diagnostic> {
        let targets = self
            .control_targets
            .iter_mut()
            .rev()
            .find_map(|(active, targets)| (*active == id).then_some(targets))
            .ok_or_else(|| {
                Diagnostic::backend(format!(
                    "break references inactive HIR control target {}",
                    id.0
                ))
            })?;
        targets.break_used = true;
        Ok(targets.break_target)
    }

    pub(super) fn continue_block(&self, id: ControlTargetId) -> Result<BasicBlockId, Diagnostic> {
        let targets = self
            .control_targets
            .iter()
            .rev()
            .find_map(|(active, targets)| (*active == id).then_some(targets))
            .ok_or_else(|| {
                Diagnostic::backend(format!(
                    "continue references inactive HIR control target {}",
                    id.0
                ))
            })?;
        match targets.kind {
            ControlTargetKind::Loop { continue_target } => Ok(continue_target),
            ControlTargetKind::BreakOnly => Err(Diagnostic::backend(format!(
                "continue references break-only HIR control target {}",
                id.0
            ))),
        }
    }

    pub(super) fn lower_breakable(
        &mut self,
        id: ControlTargetId,
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(source);
        let exit = self.new_block(provenance.clone());
        self.enter_break_target(id, exit)?;
        self.lower_block(body)?;
        let body_flows = !self.is_terminated(self.current)?;
        if body_flows {
            self.terminate(make_terminator(
                TerminatorKind::Goto(exit),
                hir::Effects::default(),
                provenance.clone(),
            ))?;
        }
        let targets = self.exit_control_target(id)?;
        self.current = exit;
        if !body_flows && !targets.break_used {
            self.terminate(make_terminator(
                TerminatorKind::Unreachable,
                hir::Effects::default(),
                provenance,
            ))?;
        }
        Ok(())
    }
}
