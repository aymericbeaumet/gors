//! Evaluation-order-explicit lowering from typed HIR to MIR.

mod append;
mod arrays;
mod assignments;
mod calls;
mod closures;
mod control_targets;
mod expressions;
mod flow;
mod goroutines;
mod interfaces;
mod maps;
mod method_receivers;
mod panic_cleanup;
mod pointers;
mod ranges;
mod slices;
mod statics;
mod structs;
#[cfg(test)]
mod test_file;

use super::construct::{make_rvalue, make_statement, make_terminator};
use super::labels::{collect_labels, statement_declares_label};
use super::{
    BasicBlock, Function, LocalDecl, Operand, Place, Provenance, RvalueKind, Statement,
    SyntheticOrigin, Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, ClosureId, ControlTargetId, LocalId};
use crate::compiler::types::{ConstValue, Ty};
use panic_cleanup::DeferredCall;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
pub(super) use test_file::lower_file;

struct BlockBuilder {
    provenance: Provenance,
    statements: Vec<Statement>,
    terminator: Option<Terminator>,
}

struct FunctionLowerer {
    locals: Vec<LocalDecl>,
    blocks: Vec<BlockBuilder>,
    current: BasicBlockId,
    control_targets: Vec<(ControlTargetId, control_targets::ControlTargets)>,
    labels: BTreeMap<String, BasicBlockId>,
    named_results: Vec<Option<LocalId>>,
    closures: Vec<hir::Closure>,
    closure_returns: Vec<ClosureReturn>,
    active_closures: Vec<ClosureId>,
    deferred: Vec<DeferredCall>,
    all_deferred: Vec<DeferredCall>,
    defer_flags: Vec<LocalId>,
    next_defer: usize,
    recover_active: Option<LocalId>,
    recover_value: Option<LocalId>,
    addressed_locals: BTreeMap<LocalId, LocalId>,
    initialized_addressed_locals: BTreeSet<LocalId>,
}

#[derive(Clone)]
struct ClosureReturn {
    destinations: Vec<Place>,
    target: BasicBlockId,
    named_results: Vec<Option<LocalId>>,
}

/// Lower one independently tracked HIR function into explicit-order Go MIR.
///
/// Cross-function call validation deliberately remains a separate operation:
/// callers supply the package signature index to the MIR verifier after this
/// function-local construction step.
pub(super) fn lower_function(function: &hir::Function) -> Result<Function, Diagnostic> {
    FunctionLowerer::lower(function)
}

impl FunctionLowerer {
    fn lower(hir: &hir::Function) -> Result<Function, Diagnostic> {
        let mut locals = hir
            .locals
            .iter()
            .map(|local| LocalDecl {
                id: local.id,
                name: local.name.clone(),
                ty: local.ty.clone(),
                kind: local.kind,
            })
            .collect::<Vec<_>>();
        let addressed_locals = pointers::plan_addressed_locals(hir, &mut locals)?;
        let mut lowerer = Self {
            locals,
            blocks: vec![BlockBuilder {
                provenance: Provenance::Source(hir.body.source),
                statements: Vec::new(),
                terminator: None,
            }],
            current: BasicBlockId(0),
            control_targets: Vec::new(),
            labels: BTreeMap::new(),
            named_results: hir.named_results.clone(),
            closures: hir.closures.clone(),
            closure_returns: Vec::new(),
            active_closures: Vec::new(),
            deferred: Vec::new(),
            all_deferred: Vec::new(),
            defer_flags: Vec::new(),
            next_defer: 0,
            recover_active: None,
            recover_value: None,
            addressed_locals,
            initialized_addressed_locals: BTreeSet::new(),
        };

        let deferred_bodies = hir
            .body
            .stmts
            .iter()
            .filter_map(|statement| match &statement.kind {
                hir::StmtKind::Defer { body, .. } => Some((body.clone(), statement.source)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut panic_dispatch = None;
        let body_entry = if deferred_bodies.is_empty() {
            None
        } else {
            for (body, source) in deferred_bodies {
                let registered = lowerer.new_temp(Ty::Bool);
                lowerer.defer_flags.push(registered);
                lowerer.all_deferred.push(DeferredCall {
                    registered,
                    body,
                    source,
                });
            }
            let active = lowerer.new_temp(Ty::Bool);
            let recovered = lowerer.new_temp(Ty::Interface(Vec::new()));
            lowerer.recover_active = Some(active);
            lowerer.recover_value = Some(recovered);
            lowerer.initialize_panic_cleanup_locals(hir)?;
            panic_dispatch = Some(lowerer.current);
            let body_entry = lowerer.new_block(Provenance::Source(hir.body.source));
            lowerer.current = body_entry;
            Some(body_entry)
        };

        let mut labels = Vec::new();
        collect_labels(&hir.body, &mut labels);
        for (label, source) in labels {
            let target = lowerer.new_block(Provenance::Source(source));
            if lowerer.labels.insert(label.clone(), target).is_some() {
                return Err(Diagnostic::backend(format!(
                    "duplicate HIR label {label} reached MIR lowering"
                )));
            }
        }

        lowerer.initialize_addressed_parameters(&hir.params, hir.source)?;

        // Named Go results exist and contain their zero values at function
        // entry, even when the function exits via a bare return.
        for result in hir.named_results.iter().flatten() {
            if body_entry.is_none() || lowerer.addressed_locals.contains_key(result) {
                let ty = lowerer.local_ty(*result)?.clone();
                lowerer.lower_zero_value(
                    Place { local: *result },
                    ty,
                    Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization),
                )?;
            }
        }

        lowerer.lower_block(&hir.body)?;
        if !lowerer.is_terminated(lowerer.current)? {
            if hir.signature.results.is_empty() {
                lowerer.lower_deferred()?;
                if !lowerer.is_terminated(lowerer.current)? {
                    lowerer.terminate(make_terminator(
                        TerminatorKind::Return(Vec::new()),
                        hir::Effects::default(),
                        Provenance::Synthetic(SyntheticOrigin::ImplicitReturn),
                    ))?;
                }
            } else {
                return Err(Diagnostic::backend(format!(
                    "function {} can reach its end without returning",
                    hir.name
                )));
            }
        }

        let panic_cleanup = if let (Some(body_entry), Some(active), Some(recovered)) =
            (body_entry, lowerer.recover_active, lowerer.recover_value)
        {
            let cleanup = lowerer.build_panic_cleanup(hir, active, recovered)?;
            lowerer.retarget_body_panics(cleanup.entry);
            lowerer.current = panic_dispatch
                .ok_or_else(|| Diagnostic::backend("defer cleanup has no entry dispatch block"))?;
            lowerer.terminate(make_terminator(
                TerminatorKind::SwitchBool {
                    condition: Operand::Constant(ConstValue::Bool(false), Ty::Bool),
                    then_target: cleanup.entry,
                    else_target: body_entry,
                },
                hir::Effects::default(),
                Provenance::Synthetic(SyntheticOrigin::PanicCleanupDispatch),
            ))?;
            Some(cleanup)
        } else {
            None
        };

        let blocks = lowerer
            .blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| {
                Ok(BasicBlock {
                    id: BasicBlockId(index as u32),
                    provenance: block.provenance,
                    statements: block.statements,
                    terminator: block.terminator.ok_or_else(|| {
                        Diagnostic::backend(format!(
                            "unterminated MIR block {index} in {}",
                            hir.name
                        ))
                    })?,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;

        Ok(Function {
            id: hir.id,
            name: hir.name.clone(),
            signature: hir.signature.clone(),
            params: hir.params.clone(),
            locals: lowerer.locals,
            blocks,
            entry: BasicBlockId(0),
            panic_cleanup,
            source: hir.source,
        })
    }

    fn new_block(&mut self, provenance: Provenance) -> BasicBlockId {
        let id = BasicBlockId(self.blocks.len() as u32);
        self.blocks.push(BlockBuilder {
            provenance,
            statements: Vec::new(),
            terminator: None,
        });
        id
    }

    fn is_terminated(&self, block: BasicBlockId) -> Result<bool, Diagnostic> {
        self.blocks
            .get(block.0 as usize)
            .map(|block| block.terminator.is_some())
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR block {}", block.0)))
    }

    fn terminate(&mut self, terminator: Terminator) -> Result<(), Diagnostic> {
        let block = self
            .blocks
            .get_mut(self.current.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR block {}", self.current.0)))?;
        if block.terminator.replace(terminator).is_some() {
            return Err(Diagnostic::backend(format!(
                "MIR block {} terminated twice",
                self.current.0
            )));
        }
        Ok(())
    }

    fn push_statement(&mut self, statement: Statement) -> Result<(), Diagnostic> {
        if self.is_terminated(self.current)? {
            return Err(Diagnostic::backend(format!(
                "statement emitted after terminator in block {}",
                self.current.0
            )));
        }
        self.blocks
            .get_mut(self.current.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR block {}", self.current.0)))?
            .statements
            .push(statement);
        Ok(())
    }

    fn local_ty(&self, local: LocalId) -> Result<&Ty, Diagnostic> {
        self.locals
            .get(local.0 as usize)
            .map(|local| &local.ty)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR local {}", local.0)))
    }

    fn new_temp(&mut self, ty: Ty) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(LocalDecl {
            id,
            name: None,
            ty,
            kind: hir::LocalKind::Temporary,
        });
        id
    }

    fn materialize(
        &mut self,
        operand: Operand,
        ty: Ty,
        provenance: Provenance,
    ) -> Result<Operand, Diagnostic> {
        let local = self.new_temp(ty);
        let place = Place { local };
        let value = make_rvalue(
            RvalueKind::Use(operand),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(place, value, provenance))?;
        Ok(Operand::Read(place))
    }

    fn lower_block(&mut self, block: &hir::Block) -> Result<(), Diagnostic> {
        for statement in &block.stmts {
            if self.is_terminated(self.current)? && !statement_declares_label(statement) {
                continue;
            }
            self.lower_statement(statement)?;
        }
        Ok(())
    }

    fn lower_statement(&mut self, statement: &hir::Stmt) -> Result<(), Diagnostic> {
        match &statement.kind {
            hir::StmtKind::LetTuple {
                destinations,
                value,
                coercions,
            } => self.lower_tuple_assignment(destinations, value, coercions, true)?,
            hir::StmtKind::AssignTuple {
                destinations,
                value,
                coercions,
            } => self.lower_target_tuple_assignment(
                destinations,
                value,
                coercions,
                statement.source,
            )?,
            hir::StmtKind::Let {
                destinations,
                values,
            } => self.lower_local_assignments(destinations, values, statement.source, true)?,
            hir::StmtKind::Assign {
                destinations,
                op,
                values,
            } => self.lower_target_assignment(destinations, *op, values, statement.source)?,
            hir::StmtKind::Expr(expr) => {
                let _ = self.lower_expr(expr)?;
            }
            hir::StmtKind::ClosureBinding(_) => {}
            hir::StmtKind::Defer {
                parameters,
                values,
                body,
            } => self.register_defer(parameters, values, body, statement.source)?,
            hir::StmtKind::Go {
                parameters,
                values,
                body,
            } => self.lower_empty_goroutine(parameters, values, body, statement.source)?,
            hir::StmtKind::Return(values) => {
                if self.closure_returns.is_empty() {
                    self.lower_return(values, statement.source)?;
                } else {
                    self.lower_closure_return(values, statement.source)?;
                }
            }
            hir::StmtKind::Block(block) => self.lower_block(block)?,
            hir::StmtKind::If {
                init,
                condition,
                then_block,
                else_branch,
            } => {
                if let Some(init) = init {
                    self.lower_statement(init)?;
                }
                let condition_provenance = Provenance::Source(condition.source);
                let condition = self.lower_expr(condition)?;
                let condition = self.materialize(condition, Ty::Bool, condition_provenance)?;
                let provenance = Provenance::Source(statement.source);
                let then_target = self.new_block(provenance.clone());
                let else_target = self.new_block(provenance.clone());
                let join_target = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::SwitchBool {
                        condition,
                        then_target,
                        else_target,
                    },
                    hir::Effects::default(),
                    provenance.clone(),
                ))?;

                self.current = then_target;
                self.lower_block(then_block)?;
                let then_flows = !self.is_terminated(self.current)?;
                if then_flows {
                    self.terminate(make_terminator(
                        TerminatorKind::Goto(join_target),
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                }

                self.current = else_target;
                if let Some(else_branch) = else_branch {
                    self.lower_statement(else_branch)?;
                }
                let else_flows = !self.is_terminated(self.current)?;
                if else_flows {
                    self.terminate(make_terminator(
                        TerminatorKind::Goto(join_target),
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                }
                self.current = join_target;
                if !then_flows && !else_flows {
                    self.terminate(make_terminator(
                        TerminatorKind::Unreachable,
                        hir::Effects::default(),
                        provenance,
                    ))?;
                }
            }
            hir::StmtKind::For {
                target,
                label,
                init,
                condition,
                post,
                body,
            } => {
                if let Some(label) = label {
                    self.enter_label(label, statement.source)?;
                }
                if let Some(init) = init {
                    self.lower_statement(init)?;
                }
                let provenance = Provenance::Source(statement.source);
                let header = self.new_block(provenance.clone());
                let body_target = self.new_block(provenance.clone());
                let post_target = self.new_block(provenance.clone());
                let exit_target = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::Goto(header),
                    hir::Effects::default(),
                    provenance.clone(),
                ))?;

                self.current = header;
                if let Some(condition) = condition {
                    let condition_provenance = Provenance::Source(condition.source);
                    let condition = self.lower_expr(condition)?;
                    let condition = self.materialize(condition, Ty::Bool, condition_provenance)?;
                    self.terminate(make_terminator(
                        TerminatorKind::SwitchBool {
                            condition,
                            then_target: body_target,
                            else_target: exit_target,
                        },
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                } else {
                    self.terminate(make_terminator(
                        TerminatorKind::Goto(body_target),
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                }

                self.enter_loop_target(*target, exit_target, post_target)?;
                self.current = body_target;
                self.lower_block(body)?;
                if !self.is_terminated(self.current)? {
                    self.terminate(make_terminator(
                        TerminatorKind::Goto(post_target),
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                }

                self.current = post_target;
                if let Some(post) = post {
                    self.lower_statement(post)?;
                }
                if !self.is_terminated(self.current)? {
                    self.terminate(make_terminator(
                        TerminatorKind::Goto(header),
                        hir::Effects::default(),
                        provenance.clone(),
                    ))?;
                }
                let break_used = self.exit_control_target(*target)?.break_used;
                self.current = exit_target;
                if condition.is_none() && !break_used {
                    self.terminate(make_terminator(
                        TerminatorKind::Unreachable,
                        hir::Effects::default(),
                        provenance,
                    ))?;
                }
            }
            hir::StmtKind::Range {
                target,
                label,
                bindings,
                expression,
                body,
            } => self.lower_range(
                *target,
                label.as_deref(),
                bindings,
                expression,
                body,
                statement.source,
            )?,
            hir::StmtKind::Breakable { target, body } => {
                self.lower_breakable(*target, body, statement.source)?;
            }
            hir::StmtKind::Label {
                name,
                statement: body,
            } => {
                self.enter_label(name, statement.source)?;
                if let Some(body) = body {
                    self.lower_statement(body)?;
                }
            }
            hir::StmtKind::Goto(label) => {
                let target = self.label_target(label)?;
                self.terminate(make_terminator(
                    TerminatorKind::Goto(target),
                    hir::Effects::default(),
                    Provenance::Source(statement.source),
                ))?;
            }
            hir::StmtKind::Break(control_target) => {
                let target = self.break_block(*control_target)?;
                self.terminate(make_terminator(
                    TerminatorKind::Goto(target),
                    hir::Effects::default(),
                    Provenance::Source(statement.source),
                ))?;
            }
            hir::StmtKind::Continue(control_target) => {
                let target = self.continue_block(*control_target)?;
                self.terminate(make_terminator(
                    TerminatorKind::Goto(target),
                    hir::Effects::default(),
                    Provenance::Source(statement.source),
                ))?;
            }
        }
        Ok(())
    }

    fn lower_expr(&mut self, expr: &hir::Expr) -> Result<Operand, Diagnostic> {
        match &expr.kind {
            hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) => {
                Ok(Operand::Constant(value.clone(), expr.ty.clone()))
            }
            hir::ExprKind::GlobalVariable(_, value) => {
                self.lower_static_value(value, &expr.ty, expr.source)
            }
            hir::ExprKind::SliceLiteralI64(_)
            | hir::ExprKind::DynamicSliceLiteralI64(_)
            | hir::ExprKind::AggregateSliceLiteral { .. }
            | hir::ExprKind::AggregateSliceIndex { .. }
            | hir::ExprKind::SliceLiteralU8(_)
            | hir::ExprKind::SliceLiteralBool(_)
            | hir::ExprKind::SliceLiteralGoString(_) => self.lower_slice_expr(expr),
            hir::ExprKind::ArrayLiteralI64(elements) => {
                self.lower_array_literal_expr(elements, &expr.ty, expr.source)
            }
            hir::ExprKind::ArrayLiteral(elements) => {
                self.lower_scalar_array_literal_expr(elements, &expr.ty, expr.source)
            }
            hir::ExprKind::ArrayIndexI64 { array, index } => {
                self.lower_array_index_expr(array, index, &expr.ty, expr.source)
            }
            hir::ExprKind::ArrayIndex { array, index } => {
                self.lower_scalar_array_index_expr(array, index, &expr.ty, expr.source)
            }
            hir::ExprKind::ArrayLen { array, length } => {
                self.lower_array_len_expr(array, *length, expr.source)
            }
            hir::ExprKind::StructLiteral(fields) => {
                self.lower_struct_literal_expr(fields, &expr.ty, expr.source)
            }
            hir::ExprKind::StructField { structure, field } => {
                self.lower_struct_field_expr(structure, *field, &expr.ty, expr.source)
            }
            hir::ExprKind::MapLiteralStringI64(entries)
            | hir::ExprKind::MapLiteralI64GoString(entries) => {
                self.lower_map_literal(entries, &expr.ty, expr.source)
            }
            hir::ExprKind::AggregateMapLiteral {
                entries,
                type_identity,
            } => self.lower_aggregate_map_literal(entries, type_identity, &expr.ty, expr.source),
            hir::ExprKind::AggregateMapIndex {
                map,
                key,
                type_identity,
            } => self.lower_aggregate_map_index(map, key, type_identity, &expr.ty, expr.source),
            hir::ExprKind::Append { slice, arguments } => {
                self.lower_append_expr(slice, arguments, &expr.ty, expr.source)
            }
            hir::ExprKind::Local(local) => self.read_semantic_local(*local, expr.source),
            hir::ExprKind::AddressOfLocal(local) => {
                self.lower_address_of_local_expr(*local, &expr.ty)
            }
            hir::ExprKind::AddressOfValue(value) => {
                self.lower_address_of_value_expr(value, &expr.ty, expr.source)
            }
            hir::ExprKind::PointerStructValue(pointer) => {
                self.lower_pointer_struct_value_expr(pointer, &expr.ty, expr.source)
            }
            hir::ExprKind::MethodReceiver { receiver, plan } => {
                self.lower_method_receiver_expr(receiver, plan, &expr.ty, expr.source)
            }
            hir::ExprKind::InterfaceValue {
                value,
                type_identity,
            } => self.lower_interface_value_expr(value, type_identity, &expr.ty, expr.source),
            hir::ExprKind::InterfaceCall {
                receiver,
                args,
                candidates,
            } => self.lower_interface_call_expr(receiver, args, candidates, &expr.ty, expr.source),
            hir::ExprKind::Recover => self.lower_recover_expr(&expr.ty, expr.effects, expr.source),
            hir::ExprKind::Unary { op, operand } => {
                self.lower_unary_expr(*op, operand, &expr.ty, expr.source)
            }
            hir::ExprKind::Conversion { value } => {
                self.lower_conversion_expr(value, &expr.ty, expr.source)
            }
            hir::ExprKind::Binary { op, left, right }
                if matches!(op, hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr) =>
            {
                self.lower_short_circuit(*op, left, right, expr.source)
            }
            hir::ExprKind::Binary { op, left, right } => {
                self.lower_binary_expr(*op, left, right, &expr.ty, expr.source)
            }
            hir::ExprKind::Call { callee, args } => {
                self.lower_call_expr(*callee, args, &expr.ty, expr.source)
            }
            hir::ExprKind::ForwardedCall {
                callee,
                prefix,
                source_call,
                coercions,
                fixed_results,
                variadic_slice,
            } => self.lower_forwarded_call_expr(
                *callee,
                prefix,
                source_call,
                coercions,
                *fixed_results,
                variadic_slice.as_ref(),
                &expr.ty,
                expr.source,
            ),
        }
    }
}
