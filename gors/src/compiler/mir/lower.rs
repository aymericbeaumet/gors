//! Evaluation-order-explicit lowering from typed HIR to MIR.

mod arrays;
mod assignments;
mod closures;
mod expressions;
mod flow;
mod goroutines;
mod interfaces;
mod maps;
mod panic_cleanup;
mod pointers;
mod ranges;
mod slices;
mod statics;
mod structs;
#[cfg(test)]
mod test_file;

use super::construct::{
    assignment_binary_op, binary_effects, call_effects, make_rvalue, make_statement,
    make_terminator, operand_ty,
};
use super::labels::{collect_labels, statement_declares_label};
use super::{
    BasicBlock, Function, LocalDecl, Operand, Place, Provenance, RvalueKind, Statement,
    SyntheticOrigin, Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, ClosureId, LocalId};
use crate::compiler::types::{ConstValue, Ty};
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
    loops: Vec<LoopTargets>,
    labels: BTreeMap<String, BasicBlockId>,
    named_results: Vec<Option<LocalId>>,
    closures: Vec<hir::Closure>,
    closure_returns: Vec<ClosureReturn>,
    active_closures: Vec<ClosureId>,
    deferred: Vec<hir::Block>,
    all_deferred: Vec<DeferredAction>,
    defer_flags: Vec<LocalId>,
    next_defer: usize,
    recover_active: Option<LocalId>,
    addressed_locals: BTreeMap<LocalId, LocalId>,
    initialized_addressed_locals: BTreeSet<LocalId>,
}

#[derive(Clone)]
struct DeferredAction {
    registered: LocalId,
    body: hir::Block,
}

#[derive(Clone)]
struct ClosureReturn {
    destinations: Vec<Place>,
    target: BasicBlockId,
    named_results: Vec<Option<LocalId>>,
}

#[derive(Clone)]
struct LoopTargets {
    label: Option<String>,
    break_target: BasicBlockId,
    continue_target: BasicBlockId,
    break_used: bool,
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
            loops: Vec::new(),
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
            addressed_locals,
            initialized_addressed_locals: BTreeSet::new(),
        };

        let deferred_bodies = hir
            .body
            .stmts
            .iter()
            .filter_map(|statement| match &statement.kind {
                hir::StmtKind::Defer { body, .. } => Some(body.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut panic_dispatch = None;
        let body_entry = if deferred_bodies.is_empty() {
            None
        } else {
            for body in deferred_bodies {
                let registered = lowerer.new_temp(Ty::Bool);
                lowerer.defer_flags.push(registered);
                lowerer
                    .all_deferred
                    .push(DeferredAction { registered, body });
            }
            let active = lowerer.new_temp(Ty::Bool);
            lowerer.recover_active = Some(active);
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

        let panic_cleanup = if let (Some(body_entry), Some(active)) =
            (body_entry, lowerer.recover_active)
        {
            let cleanup = lowerer.build_panic_cleanup(hir, active)?;
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
            } => self.lower_tuple_assignment(destinations, value, coercions, false)?,
            hir::StmtKind::ParallelAssignTuple {
                destinations,
                value,
                coercions,
            } => self.lower_parallel_tuple_assignment(
                destinations,
                value,
                coercions,
                statement.source,
            )?,
            hir::StmtKind::ParallelAssign {
                destinations,
                values,
            } => self.lower_parallel_assignment(destinations, values, statement.source)?,
            hir::StmtKind::Let {
                destinations,
                values,
            } => self.lower_local_assignments(destinations, values, statement.source, true)?,
            hir::StmtKind::Assign {
                destinations,
                op: hir::AssignOp::Set,
                values,
            } => self.lower_local_assignments(destinations, values, statement.source, false)?,
            hir::StmtKind::Assign {
                destinations,
                op,
                values,
            } => {
                let [hir::Place::Local(destination)] = destinations.as_slice() else {
                    return Err(Diagnostic::backend(
                        "compound assignment must have one local destination",
                    ));
                };
                let [value] = values.as_slice() else {
                    return Err(Diagnostic::backend(
                        "compound assignment must have one operand",
                    ));
                };
                self.lower_compound_local_assignment(*destination, *op, value, statement.source)?;
            }
            hir::StmtKind::SliceAssign {
                slice,
                index,
                set,
                op,
                value,
            } => {
                let provenance = Provenance::Source(statement.source);
                let slice_operand = self.lower_expr(slice)?;
                let slice_operand = self.materialize(
                    slice_operand,
                    slice.ty.clone(),
                    Provenance::Source(slice.source),
                )?;
                let index_operand = self.lower_expr(index)?;
                let index_operand = self.materialize(
                    index_operand,
                    index.ty.clone(),
                    Provenance::Source(index.source),
                )?;

                let assigned = if *op == hir::AssignOp::Set {
                    let value_operand = self.lower_expr(value)?;
                    self.materialize(
                        value_operand,
                        value.ty.clone(),
                        Provenance::Source(value.source),
                    )?
                } else {
                    let get = match set {
                        hir::Builtin::SliceI64Set => hir::Builtin::SliceI64Index,
                        hir::Builtin::SliceBoolSet => hir::Builtin::SliceBoolIndex,
                        _ => {
                            return Err(Diagnostic::backend(
                                "slice assignment selected a non-slice runtime operation",
                            ));
                        }
                    };
                    let old = Place {
                        local: self.new_temp(value.ty.clone()),
                    };
                    let after_index = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(get),
                            args: vec![slice_operand.clone(), index_operand.clone()],
                            destinations: vec![old],
                            target: after_index,
                        },
                        call_effects(),
                        provenance.clone(),
                    ))?;
                    self.current = after_index;
                    let value_operand = self.lower_expr(value)?;
                    let value_operand = self.materialize(
                        value_operand,
                        value.ty.clone(),
                        Provenance::Source(value.source),
                    )?;
                    let result = Place {
                        local: self.new_temp(value.ty.clone()),
                    };
                    let binary_op = assignment_binary_op(*op);
                    let binary = make_rvalue(
                        RvalueKind::Binary {
                            op: binary_op,
                            left: Operand::Read(old),
                            right: value_operand,
                            ty: value.ty.clone(),
                        },
                        binary_effects(binary_op, &value.ty),
                        provenance.clone(),
                    );
                    self.push_statement(make_statement(result, binary, provenance.clone()))?;
                    Operand::Read(result)
                };

                let after_set = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: hir::Callee::Builtin(*set),
                        args: vec![slice_operand, index_operand, assigned],
                        destinations: Vec::new(),
                        target: after_set,
                    },
                    call_effects(),
                    provenance,
                ))?;
                self.current = after_set;
            }
            hir::StmtKind::ArrayAssign {
                array,
                index,
                op,
                value,
            } => {
                self.lower_array_assignment_stmt(*array, index, *op, value, statement.source)?;
            }
            hir::StmtKind::StructFieldAssign {
                structure,
                field,
                op,
                value,
            } => {
                self.lower_struct_field_assignment_stmt(
                    *structure,
                    *field,
                    *op,
                    value,
                    statement.source,
                )?;
            }
            hir::StmtKind::MapAssign { map, key, value } => {
                self.lower_map_assignment(map, key, value, statement.source)?;
            }
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

                self.loops.push(LoopTargets {
                    label: label.clone(),
                    break_target: exit_target,
                    continue_target: post_target,
                    break_used: false,
                });
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
                let break_used = self.loops.pop().is_some_and(|targets| targets.break_used);
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
                label,
                key,
                value,
                expression,
                body,
            } => self.lower_range(
                label.as_deref(),
                *key,
                *value,
                expression,
                body,
                statement.source,
            )?,
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
            hir::StmtKind::Break(label) => {
                let targets = match label {
                    Some(label) => self
                        .loops
                        .iter_mut()
                        .rev()
                        .find(|targets| targets.label.as_deref() == Some(label)),
                    None => self.loops.last_mut(),
                }
                .ok_or_else(|| Diagnostic::backend("break outside MIR loop"))?;
                targets.break_used = true;
                let target = targets.break_target;
                self.terminate(make_terminator(
                    TerminatorKind::Goto(target),
                    hir::Effects::default(),
                    Provenance::Source(statement.source),
                ))?;
            }
            hir::StmtKind::Continue(label) => {
                let target = match label {
                    Some(label) => self
                        .loops
                        .iter()
                        .rev()
                        .find(|targets| targets.label.as_deref() == Some(label)),
                    None => self.loops.last(),
                }
                .ok_or_else(|| Diagnostic::backend("continue outside MIR loop"))?
                .continue_target;
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
            hir::ExprKind::SliceLiteralI64(elements) => {
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralI64(elements.clone()),
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, value, provenance))?;
                Ok(Operand::Read(place))
            }
            hir::ExprKind::DynamicSliceLiteralI64(elements) => {
                self.lower_dynamic_i64_slice_literal(elements, &expr.ty, expr.source)
            }
            hir::ExprKind::SliceLiteralU8(elements) => {
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralU8(elements.clone()),
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, value, provenance))?;
                Ok(Operand::Read(place))
            }
            hir::ExprKind::SliceLiteralBool(elements) => {
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralBool(elements.clone()),
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, value, provenance))?;
                Ok(Operand::Read(place))
            }
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
            hir::ExprKind::MapLiteralStringI64(entries) => {
                self.lower_map_literal(entries, &expr.ty, expr.source)
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
            hir::ExprKind::InterfaceValue {
                value,
                type_identity,
            } => self.lower_interface_value_expr(value, type_identity, &expr.ty, expr.source),
            hir::ExprKind::InterfaceCall {
                receiver,
                args,
                candidates,
            } => self.lower_interface_call_expr(receiver, args, candidates, &expr.ty, expr.source),
            hir::ExprKind::RecoverCompareNil { equal } => {
                let state = self.recover_active.ok_or_else(|| {
                    Diagnostic::backend("recover comparison reached a function without cleanup")
                })?;
                let result = Place {
                    local: self.new_temp(Ty::Bool),
                };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::RecoverCompareNil {
                        state: Place { local: state },
                        equal: *equal,
                    },
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            hir::ExprKind::Unary { op, operand } => {
                let operand_provenance = Provenance::Source(operand.source);
                let operand = self.lower_expr(operand)?;
                let ty = operand_ty(&operand, &self.locals)?;
                let operand = self.materialize(operand, ty, operand_provenance)?;
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::Unary {
                        op: *op,
                        operand,
                        ty: expr.ty.clone(),
                    },
                    hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, value, provenance))?;
                Ok(Operand::Read(place))
            }
            hir::ExprKind::Conversion { value } => {
                let operand = self.lower_expr(value)?;
                let operand =
                    self.materialize(operand, value.ty.clone(), Provenance::Source(value.source))?;
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let converted = make_rvalue(
                    RvalueKind::Conversion {
                        operand,
                        from: value.ty.clone(),
                        ty: expr.ty.clone(),
                    },
                    value.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, converted, provenance))?;
                Ok(Operand::Read(place))
            }
            hir::ExprKind::Binary { op, left, right }
                if matches!(op, hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr) =>
            {
                self.lower_short_circuit(*op, left, right, expr.source)
            }
            hir::ExprKind::Binary { op, left, right } => {
                // Each operand is frozen immediately after its evaluation;
                // evaluation of the right operand cannot observe a delayed
                // read of the left operand.
                let left_operand = self.lower_expr(left)?;
                let left_operand = self.materialize(
                    left_operand,
                    left.ty.clone(),
                    Provenance::Source(left.source),
                )?;
                let right_operand = self.lower_expr(right)?;
                let right_operand = self.materialize(
                    right_operand,
                    right.ty.clone(),
                    Provenance::Source(right.source),
                )?;
                let result = self.new_temp(expr.ty.clone());
                let place = Place { local: result };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::Binary {
                        op: *op,
                        left: left_operand,
                        right: right_operand,
                        ty: expr.ty.clone(),
                    },
                    binary_effects(*op, &expr.ty),
                    provenance.clone(),
                );
                self.push_statement(make_statement(place, value, provenance))?;
                Ok(Operand::Read(place))
            }
            hir::ExprKind::Call { callee, args } => {
                if *callee == hir::Callee::Builtin(hir::Builtin::InterfaceAssert) {
                    return self.lower_interface_assertion_expr(args, &expr.ty, expr.source);
                }
                if let hir::Callee::Closure(id) = callee {
                    if matches!(expr.ty, Ty::Tuple(_)) {
                        return Err(Diagnostic::backend(
                            "tuple-valued local function call bypassed tuple lowering",
                        ));
                    }
                    let destination = (expr.ty != Ty::Unit).then(|| Place {
                        local: self.new_temp(expr.ty.clone()),
                    });
                    self.lower_closure_call(
                        *id,
                        args,
                        destination.into_iter().collect(),
                        expr.source,
                    )?;
                    return Ok(destination.map_or(Operand::Unit, Operand::Read));
                }
                let mut operands = Vec::new();
                for arg in args {
                    let operand = self.lower_expr(arg)?;
                    operands.push(self.materialize(
                        operand,
                        arg.ty.clone(),
                        Provenance::Source(arg.source),
                    )?);
                }
                let provenance = Provenance::Source(expr.source);
                let target = self.new_block(provenance.clone());
                let destination = (expr.ty != Ty::Unit).then(|| Place {
                    local: self.new_temp(expr.ty.clone()),
                });
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: *callee,
                        args: operands,
                        destinations: destination.into_iter().collect(),
                        target,
                    },
                    call_effects(),
                    provenance,
                ))?;
                self.current = target;
                Ok(destination.map_or(Operand::Unit, Operand::Read))
            }
        }
    }
}
