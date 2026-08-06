//! MIR storage planning for address-taken integers and integer structs.

use std::collections::{BTreeMap, BTreeSet};

use super::super::construct::{
    assignment_binary_op, binary_effects, call_effects, make_rvalue, make_statement,
    make_terminator,
};
use super::super::{LocalDecl, Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

pub(super) fn plan_addressed_locals(
    function: &hir::Function,
    locals: &mut Vec<LocalDecl>,
) -> Result<BTreeMap<LocalId, LocalId>, Diagnostic> {
    let mut addressed = BTreeSet::new();
    collect_block_addresses(&function.body, &mut addressed);
    for closure in &function.closures {
        collect_block_addresses(&closure.body, &mut addressed);
    }

    let mut plan = BTreeMap::new();
    for local in addressed {
        let declaration = locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("addressed unknown local {}", local.0)))?;
        if declaration.ty.underlying() != &Ty::Int(IntTy::Int)
            && declaration.ty.bootstrap_i64_struct_fields().is_none()
        {
            return Err(Diagnostic::backend(format!(
                "addressed local {} has unsupported type {:?}",
                local.0, declaration.ty
            )));
        }
        let pointer_id = LocalId(
            u32::try_from(locals.len())
                .map_err(|_| Diagnostic::backend("function exceeds the MIR local ID space"))?,
        );
        locals.push(LocalDecl {
            id: pointer_id,
            name: None,
            ty: Ty::Pointer(Box::new(declaration.ty.clone())),
            kind: hir::LocalKind::Temporary,
        });
        plan.insert(local, pointer_id);
    }
    Ok(plan)
}

impl FunctionLowerer {
    pub(super) fn initialize_addressed_parameters(
        &mut self,
        parameters: &[LocalId],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let addressed = parameters
            .iter()
            .copied()
            .filter(|local| self.addressed_locals.contains_key(local))
            .collect::<Vec<_>>();
        for local in addressed {
            self.write_semantic_local(
                local,
                Operand::Read(Place { local }),
                Provenance::Source(source),
                true,
            )?;
        }
        Ok(())
    }

    pub(super) fn lower_address_of_local_expr(
        &self,
        local: LocalId,
        ty: &Ty,
    ) -> Result<Operand, Diagnostic> {
        let pointer = self.addressed_locals.get(&local).copied().ok_or_else(|| {
            Diagnostic::backend(format!("local {} has no addressable MIR storage", local.0))
        })?;
        if !self.initialized_addressed_locals.contains(&local) {
            return Err(Diagnostic::backend(format!(
                "addressed local {} is used before storage initialization",
                local.0
            )));
        }
        if self.local_ty(pointer)? != ty {
            return Err(Diagnostic::backend(format!(
                "addressed local {} has mismatched pointer type",
                local.0
            )));
        }
        Ok(Operand::Read(Place { local: pointer }))
    }

    pub(super) fn read_semantic_local(
        &mut self,
        local: LocalId,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Some(pointer) = self.addressed_locals.get(&local).copied() else {
            return Ok(Operand::Read(Place { local }));
        };
        if !self.initialized_addressed_locals.contains(&local) {
            return Err(Diagnostic::backend(format!(
                "addressed local {} is read before storage initialization",
                local.0
            )));
        }
        let ty = self.local_ty(local)?.clone();
        if ty.underlying() == &Ty::Int(IntTy::Int) {
            let result = Place {
                local: self.new_temp(ty),
            };
            self.emit_pointer_call(
                hir::Builtin::PointerI64Get,
                vec![Operand::Read(Place { local: pointer })],
                vec![result],
                Provenance::Source(source),
            )?;
            Ok(Operand::Read(result))
        } else if ty.bootstrap_i64_struct_fields().is_some() {
            self.read_struct_pointer_value(Operand::Read(Place { local: pointer }), &ty, source)
        } else {
            Err(Diagnostic::backend(format!(
                "addressed local {} has unsupported read type {ty:?}",
                local.0
            )))
        }
    }

    pub(super) fn write_semantic_local(
        &mut self,
        local: LocalId,
        operand: Operand,
        provenance: Provenance,
        initialize: bool,
    ) -> Result<(), Diagnostic> {
        let Some(pointer) = self.addressed_locals.get(&local).copied() else {
            let value = make_rvalue(
                RvalueKind::Use(operand),
                hir::Effects::default(),
                provenance.clone(),
            );
            return self.push_statement(make_statement(Place { local }, value, provenance));
        };
        let ty = self.local_ty(local)?.clone();
        if initialize && self.initialized_addressed_locals.insert(local) {
            if ty.underlying() == &Ty::Int(IntTy::Int) {
                self.emit_pointer_call(
                    hir::Builtin::PointerI64New,
                    Vec::new(),
                    vec![Place { local: pointer }],
                    provenance.clone(),
                )?;
            } else if ty.bootstrap_i64_struct_fields().is_some() {
                self.allocate_struct_pointer(pointer, &ty, provenance.clone())?;
            } else {
                return Err(Diagnostic::backend(format!(
                    "addressed local {} has unsupported initialization type {ty:?}",
                    local.0
                )));
            }
        } else if !self.initialized_addressed_locals.contains(&local) {
            return Err(Diagnostic::backend(format!(
                "addressed local {} is written before storage initialization",
                local.0
            )));
        }
        if ty.underlying() == &Ty::Int(IntTy::Int) {
            self.emit_pointer_call(
                hir::Builtin::PointerI64Set,
                vec![Operand::Read(Place { local: pointer }), operand],
                Vec::new(),
                provenance,
            )
        } else if ty.bootstrap_i64_struct_fields().is_some() {
            self.write_struct_pointer_fields(pointer, operand, &ty, provenance)
        } else {
            Err(Diagnostic::backend(format!(
                "addressed local {} has unsupported write type {ty:?}",
                local.0
            )))
        }
    }

    pub(super) fn lower_address_of_value_expr(
        &mut self,
        value: &hir::Expr,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Ty::Pointer(element) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "address-of-value HIR expression has a non-pointer type",
            ));
        };
        if element.bootstrap_i64_struct_fields().is_none() || value.ty != **element {
            return Err(Diagnostic::backend(
                "address-of-value HIR expression has an invalid integer struct value",
            ));
        }
        let value_operand = self.lower_expr(value)?;
        let value_operand = self.materialize(
            value_operand,
            value.ty.clone(),
            Provenance::Source(value.source),
        )?;
        let pointer = self.new_temp(ty.clone());
        let provenance = Provenance::Source(source);
        self.allocate_struct_pointer(pointer, element, provenance.clone())?;
        self.write_struct_pointer_fields(pointer, value_operand, element, provenance)?;
        Ok(Operand::Read(Place { local: pointer }))
    }

    pub(super) fn lower_pointer_struct_value_expr(
        &mut self,
        pointer: &hir::Expr,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if ty.bootstrap_i64_struct_fields().is_none() {
            return Err(Diagnostic::backend(
                "pointer-struct dereference has a non-integer-struct result",
            ));
        }
        let pointer_operand = self.lower_expr(pointer)?;
        let pointer_operand = self.materialize(
            pointer_operand,
            pointer.ty.clone(),
            Provenance::Source(pointer.source),
        )?;
        self.read_struct_pointer_value(pointer_operand, ty, source)
    }

    fn allocate_struct_pointer(
        &mut self,
        pointer: LocalId,
        ty: &Ty,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let fields = ty.bootstrap_i64_struct_fields().ok_or_else(|| {
            Diagnostic::backend("integer struct pointer allocation has a non-struct type")
        })?;
        self.emit_pointer_call(
            hir::Builtin::PointerStructI64New,
            vec![int_constant_operand(fields.len())],
            vec![Place { local: pointer }],
            provenance,
        )
    }

    pub(super) fn read_struct_pointer_value(
        &mut self,
        pointer: Operand,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let field_types = ty
            .bootstrap_i64_struct_fields()
            .ok_or_else(|| {
                Diagnostic::backend("integer struct pointer read has a non-struct type")
            })?
            .iter()
            .map(|field| field.ty.clone())
            .collect::<Vec<_>>();
        let provenance = Provenance::Source(source);
        let mut fields = Vec::with_capacity(field_types.len());
        for (field, field_ty) in field_types.into_iter().enumerate() {
            let result = Place {
                local: self.new_temp(field_ty),
            };
            self.emit_pointer_call(
                hir::Builtin::PointerStructI64Get,
                vec![pointer.clone(), int_constant_operand(field)],
                vec![result],
                provenance.clone(),
            )?;
            fields.push(Operand::Read(result));
        }
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let value = make_rvalue(
            RvalueKind::StructLiteral {
                fields,
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    fn write_struct_pointer_fields(
        &mut self,
        pointer: LocalId,
        structure: Operand,
        ty: &Ty,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let field_types = ty
            .bootstrap_i64_struct_fields()
            .ok_or_else(|| {
                Diagnostic::backend("integer struct pointer write has a non-struct type")
            })?
            .iter()
            .map(|field| field.ty.clone())
            .collect::<Vec<_>>();
        for (field, field_ty) in field_types.into_iter().enumerate() {
            let value = Place {
                local: self.new_temp(field_ty),
            };
            let field_index = u32::try_from(field)
                .map_err(|_| Diagnostic::backend("struct field index does not fit u32"))?;
            let read = make_rvalue(
                RvalueKind::StructField {
                    structure: structure.clone(),
                    field: field_index,
                },
                hir::Effects::default(),
                provenance.clone(),
            );
            self.push_statement(make_statement(value, read, provenance.clone()))?;
            self.emit_pointer_call(
                hir::Builtin::PointerStructI64Set,
                vec![
                    Operand::Read(Place { local: pointer }),
                    int_constant_operand(field),
                    Operand::Read(value),
                ],
                Vec::new(),
                provenance.clone(),
            )?;
        }
        Ok(())
    }

    pub(super) fn lower_local_assignments(
        &mut self,
        destinations: &[hir::Place],
        values: &[hir::Expr],
        source: SourceRef,
        initialize: bool,
    ) -> Result<(), Diagnostic> {
        if destinations.len() != values.len() {
            return Err(Diagnostic::backend(
                "local assignment arity changed before MIR lowering",
            ));
        }
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
                self.write_semantic_local(*local, operand, Provenance::Source(source), initialize)?;
            }
        }
        Ok(())
    }

    pub(super) fn lower_tuple_assignment(
        &mut self,
        destinations: &[hir::Place],
        value: &hir::Expr,
        initialize: bool,
    ) -> Result<(), Diagnostic> {
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::backend(
                "multi-result HIR assignment value is not a tuple",
            ));
        };
        if destinations.len() != component_types.len() {
            return Err(Diagnostic::backend(
                "multi-result HIR assignment arity changed before MIR lowering",
            ));
        }
        let temporary_results = component_types
            .iter()
            .map(|ty| Place {
                local: self.new_temp(ty.clone()),
            })
            .collect::<Vec<_>>();
        self.lower_call_into(value, temporary_results.clone())?;
        for (destination, result) in destinations.iter().zip(temporary_results) {
            if let hir::Place::Local(local) = destination {
                self.write_semantic_local(
                    *local,
                    Operand::Read(result),
                    Provenance::Source(value.source),
                    initialize,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn lower_compound_local_assignment(
        &mut self,
        destination: LocalId,
        op: hir::AssignOp,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let ty = self.local_ty(destination)?.clone();
        let old = self.read_semantic_local(destination, source)?;
        let old = self.materialize(old, ty.clone(), Provenance::Source(source))?;
        let rhs = self.lower_expr(value)?;
        let rhs = self.materialize(rhs, value.ty.clone(), Provenance::Source(value.source))?;
        let op = assignment_binary_op(op);
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Binary {
                op,
                left: old,
                right: rhs,
                ty: ty.clone(),
            },
            binary_effects(op, &ty),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.write_semantic_local(destination, Operand::Read(result), provenance, false)
    }

    pub(super) fn emit_pointer_call(
        &mut self,
        callee: hir::Builtin,
        args: Vec<Operand>,
        destinations: Vec<Place>,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(callee),
                args,
                destinations,
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(())
    }
}

pub(super) fn int_constant_operand(value: usize) -> Operand {
    Operand::Constant(ConstValue::Int(value.to_string()), Ty::Int(IntTy::Int))
}

fn collect_block_addresses(block: &hir::Block, addressed: &mut BTreeSet<LocalId>) {
    for statement in &block.stmts {
        collect_statement_addresses(statement, addressed);
    }
}

fn collect_statement_addresses(statement: &hir::Stmt, addressed: &mut BTreeSet<LocalId>) {
    match &statement.kind {
        hir::StmtKind::Let { values, .. } | hir::StmtKind::Assign { values, .. } => {
            collect_expression_addresses(values, addressed);
        }
        hir::StmtKind::LetTuple { value, .. } | hir::StmtKind::AssignTuple { value, .. } => {
            collect_expr_addresses(value, addressed);
        }
        hir::StmtKind::ParallelAssign {
            destinations,
            values,
        } => {
            for destination in destinations {
                match destination {
                    hir::AssignTarget::SliceIndex { slice, index } => {
                        collect_expr_addresses(slice, addressed);
                        collect_expr_addresses(index, addressed);
                    }
                    hir::AssignTarget::MapIndex { map, key } => {
                        collect_expr_addresses(map, addressed);
                        collect_expr_addresses(key, addressed);
                    }
                    hir::AssignTarget::Local(_) | hir::AssignTarget::Discard => {}
                }
            }
            collect_expression_addresses(values, addressed);
        }
        hir::StmtKind::Expr(expression) => collect_expr_addresses(expression, addressed),
        hir::StmtKind::ClosureBinding(_) => {}
        hir::StmtKind::Defer { values, body, .. } => {
            collect_expression_addresses(values, addressed);
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Return(values) => collect_expression_addresses(values, addressed),
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                collect_statement_addresses(init, addressed);
            }
            collect_expr_addresses(condition, addressed);
            collect_block_addresses(then_block, addressed);
            if let Some(else_branch) = else_branch {
                collect_statement_addresses(else_branch, addressed);
            }
        }
        hir::StmtKind::For {
            init,
            condition,
            post,
            body,
            ..
        } => {
            if let Some(init) = init {
                collect_statement_addresses(init, addressed);
            }
            if let Some(condition) = condition {
                collect_expr_addresses(condition, addressed);
            }
            if let Some(post) = post {
                collect_statement_addresses(post, addressed);
            }
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Range {
            expression, body, ..
        } => {
            collect_expr_addresses(expression, addressed);
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Block(block) => collect_block_addresses(block, addressed),
        hir::StmtKind::Label { statement, .. } => {
            if let Some(statement) = statement {
                collect_statement_addresses(statement, addressed);
            }
        }
        hir::StmtKind::SliceAssign {
            slice,
            index,
            value,
            ..
        } => collect_expression_addresses([slice, index, value], addressed),
        hir::StmtKind::ArrayAssign { index, value, .. } => {
            collect_expression_addresses([index, value], addressed);
        }
        hir::StmtKind::StructFieldAssign { value, .. } => {
            collect_expr_addresses(value, addressed);
        }
        hir::StmtKind::MapAssign { map, key, value } => {
            collect_expression_addresses([map, key, value], addressed);
        }
        hir::StmtKind::Goto(_) | hir::StmtKind::Break(_) | hir::StmtKind::Continue(_) => {}
    }
}

fn collect_expression_addresses<'a>(
    expressions: impl IntoIterator<Item = &'a hir::Expr>,
    addressed: &mut BTreeSet<LocalId>,
) {
    for expression in expressions {
        collect_expr_addresses(expression, addressed);
    }
}

fn collect_expr_addresses(expression: &hir::Expr, addressed: &mut BTreeSet<LocalId>) {
    match &expression.kind {
        hir::ExprKind::AddressOfLocal(local) => {
            addressed.insert(*local);
        }
        hir::ExprKind::AddressOfValue(value)
        | hir::ExprKind::PointerStructValue(value)
        | hir::ExprKind::InterfaceValue { value, .. } => {
            collect_expr_addresses(value, addressed);
        }
        hir::ExprKind::InterfaceCall { receiver, args, .. } => {
            collect_expr_addresses(receiver, addressed);
            collect_expression_addresses(args, addressed);
        }
        hir::ExprKind::Binary { left, right, .. } => {
            collect_expr_addresses(left, addressed);
            collect_expr_addresses(right, addressed);
        }
        hir::ExprKind::Unary { operand, .. } => collect_expr_addresses(operand, addressed),
        hir::ExprKind::Conversion { value } => collect_expr_addresses(value, addressed),
        hir::ExprKind::ArrayIndexI64 { array, index } => {
            collect_expr_addresses(array, addressed);
            collect_expr_addresses(index, addressed);
        }
        hir::ExprKind::ArrayIndex { array, index } => {
            collect_expr_addresses(array, addressed);
            collect_expr_addresses(index, addressed);
        }
        hir::ExprKind::ArrayLen { array, .. } => collect_expr_addresses(array, addressed),
        hir::ExprKind::StructLiteral(fields) => collect_expression_addresses(fields, addressed),
        hir::ExprKind::ArrayLiteral(elements) => {
            for (_, element) in elements {
                collect_expr_addresses(element, addressed);
            }
        }
        hir::ExprKind::StructField { structure, .. } => {
            collect_expr_addresses(structure, addressed);
        }
        hir::ExprKind::MapLiteralStringI64(entries) => {
            for (key, value) in entries {
                collect_expr_addresses(key, addressed);
                collect_expr_addresses(value, addressed);
            }
        }
        hir::ExprKind::Call { args, .. } => collect_expression_addresses(args, addressed),
        hir::ExprKind::Constant(_)
        | hir::ExprKind::Local(_)
        | hir::ExprKind::GlobalConstant(..)
        | hir::ExprKind::GlobalVariable(..)
        | hir::ExprKind::RecoverCompareNil { .. }
        | hir::ExprKind::SliceLiteralI64(_)
        | hir::ExprKind::SliceLiteralU8(_)
        | hir::ExprKind::ArrayLiteralI64(_) => {}
    }
}
