//! Evaluation-order-explicit lowering from typed HIR to MIR.

#[cfg(test)]
use super::File;
use super::construct::{
    assignment_binary_op, binary_effects, call_effects, make_rvalue, make_statement,
    make_terminator, operand_ty,
};
use super::{
    BasicBlock, Function, LocalDecl, Operand, Place, Provenance, RvalueKind, Statement,
    SyntheticOrigin, Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, LocalId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Ty};
use std::collections::BTreeMap;

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
}

#[derive(Clone)]
struct LoopTargets {
    label: Option<String>,
    break_target: BasicBlockId,
    continue_target: BasicBlockId,
    break_used: bool,
}

#[cfg(test)]
pub(super) fn lower_file(file: &hir::File) -> Result<File, Vec<Diagnostic>> {
    let mut functions = Vec::new();
    let mut diagnostics = Vec::new();
    for function in &file.functions {
        match FunctionLowerer::lower(function) {
            Ok(function) => functions.push(function),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if diagnostics.is_empty() {
        Ok(File {
            package: file.package.clone(),
            functions,
        })
    } else {
        Err(diagnostics)
    }
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
        let locals = hir
            .locals
            .iter()
            .map(|local| LocalDecl {
                id: local.id,
                name: local.name.clone(),
                ty: local.ty.clone(),
                kind: local.kind,
            })
            .collect();
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

        // Named Go results exist and contain their zero values at function
        // entry, even when the function exits via a bare return.
        for result in hir.named_results.iter().flatten() {
            let ty = lowerer.local_ty(*result)?.clone();
            let value = ty.zero().ok_or_else(|| {
                Diagnostic::backend(format!("no MIR zero value for named result {ty:?}"))
            })?;
            let provenance = Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization);
            let value = make_rvalue(
                RvalueKind::Use(Operand::Constant(value, ty)),
                hir::Effects::default(),
                provenance.clone(),
            );
            lowerer.push_statement(make_statement(Place { local: *result }, value, provenance))?;
        }

        lowerer.lower_block(&hir.body)?;
        if !lowerer.is_terminated(lowerer.current)? {
            if hir.signature.results.is_empty() {
                lowerer.terminate(make_terminator(
                    TerminatorKind::Return(Vec::new()),
                    hir::Effects::default(),
                    Provenance::Synthetic(SyntheticOrigin::ImplicitReturn),
                ))?;
            } else {
                return Err(Diagnostic::backend(format!(
                    "function {} can reach its end without returning",
                    hir.name
                )));
            }
        }

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
            hir::StmtKind::Let {
                destinations,
                values,
            }
            | hir::StmtKind::Assign {
                destinations,
                op: hir::AssignOp::Set,
                values,
            } => {
                let mut operands = Vec::with_capacity(values.len());
                for value in values {
                    let operand = self.lower_expr(value)?;
                    operands.push(self.materialize(
                        operand,
                        value.ty.clone(),
                        Provenance::Source(value.source),
                    )?);
                }
                for (destination, operand) in destinations.iter().zip(operands) {
                    if let hir::Place::Local(local) = destination {
                        let provenance = Provenance::Source(statement.source);
                        let value = make_rvalue(
                            RvalueKind::Use(operand),
                            hir::Effects::default(),
                            provenance.clone(),
                        );
                        self.push_statement(make_statement(
                            Place { local: *local },
                            value,
                            provenance,
                        ))?;
                    }
                }
            }
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
                // Materialize the old value and RHS independently.  Later
                // place projections can be prepared before either operation
                // without changing this write boundary.
                let ty = self.local_ty(*destination)?.clone();
                let old = self.materialize(
                    Operand::Read(Place {
                        local: *destination,
                    }),
                    ty.clone(),
                    Provenance::Source(statement.source),
                )?;
                let rhs = self.lower_expr(value)?;
                let rhs =
                    self.materialize(rhs, value.ty.clone(), Provenance::Source(value.source))?;
                let op = assignment_binary_op(*op);
                let effects = binary_effects(op, &ty);
                let provenance = Provenance::Source(statement.source);
                let value = make_rvalue(
                    RvalueKind::Binary {
                        op,
                        left: old,
                        right: rhs,
                        ty,
                    },
                    effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(
                    Place {
                        local: *destination,
                    },
                    value,
                    provenance,
                ))?;
            }
            hir::StmtKind::Expr(expr) => {
                let _ = self.lower_expr(expr)?;
            }
            hir::StmtKind::Return(values) => {
                let mut operands = Vec::new();
                for value in values {
                    let operand = self.lower_expr(value)?;
                    operands.push(self.materialize(
                        operand,
                        value.ty.clone(),
                        Provenance::Source(value.source),
                    )?);
                }
                self.terminate(make_terminator(
                    TerminatorKind::Return(operands),
                    hir::Effects::default(),
                    Provenance::Source(statement.source),
                ))?;
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

    fn label_target(&self, label: &str) -> Result<BasicBlockId, Diagnostic> {
        self.labels
            .get(label)
            .copied()
            .ok_or_else(|| Diagnostic::backend(format!("unknown HIR label {label}")))
    }

    fn enter_label(&mut self, label: &str, source: SourceRef) -> Result<(), Diagnostic> {
        let target = self.label_target(label)?;
        if self.current != target && !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Goto(target),
                hir::Effects::default(),
                Provenance::Source(source),
            ))?;
        }
        self.current = target;
        Ok(())
    }

    fn lower_expr(&mut self, expr: &hir::Expr) -> Result<Operand, Diagnostic> {
        match &expr.kind {
            hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) => {
                Ok(Operand::Constant(value.clone(), expr.ty.clone()))
            }
            hir::ExprKind::Local(local) => Ok(Operand::Read(Place { local: *local })),
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
                        destination,
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

    fn lower_short_circuit(
        &mut self,
        op: hir::BinaryOp,
        left: &hir::Expr,
        right: &hir::Expr,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let left = self.lower_expr(left)?;
        let left = self.materialize(left, Ty::Bool, Provenance::Source(source))?;
        let provenance = Provenance::Source(source);
        let evaluate_right = self.new_block(provenance.clone());
        let short_value = self.new_block(provenance.clone());
        let join = self.new_block(provenance.clone());
        let result = Place {
            local: self.new_temp(Ty::Bool),
        };
        let (then_target, else_target, short_constant) = match op {
            hir::BinaryOp::LogicalAnd => (evaluate_right, short_value, false),
            hir::BinaryOp::LogicalOr => (short_value, evaluate_right, true),
            _ => return Err(Diagnostic::backend("non-logical short-circuit operation")),
        };
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: left,
                then_target,
                else_target,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = short_value;
        let value = make_rvalue(
            RvalueKind::Use(Operand::Constant(
                ConstValue::Bool(short_constant),
                Ty::Bool,
            )),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = evaluate_right;
        let right = self.lower_expr(right)?;
        let right = self.materialize(right, Ty::Bool, Provenance::Source(source))?;
        let value = make_rvalue(
            RvalueKind::Use(right),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance,
        ))?;
        self.current = join;
        Ok(Operand::Read(result))
    }
}

fn statement_declares_label(statement: &hir::Stmt) -> bool {
    matches!(
        &statement.kind,
        hir::StmtKind::Label { .. } | hir::StmtKind::For { label: Some(_), .. }
    )
}

fn collect_labels(block: &hir::Block, labels: &mut Vec<(String, SourceRef)>) {
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
        | hir::StmtKind::Assign { .. }
        | hir::StmtKind::Expr(_)
        | hir::StmtKind::Return(_)
        | hir::StmtKind::Goto(_)
        | hir::StmtKind::Break(_)
        | hir::StmtKind::Continue(_) => {}
    }
}
