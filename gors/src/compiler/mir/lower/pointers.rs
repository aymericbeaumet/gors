//! MIR storage planning for address-taken integers and integer structs.

use super::super::construct::{call_effects, make_rvalue, make_statement, make_terminator};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

mod address_analysis;

pub(super) use address_analysis::plan_addressed_locals;

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
        } else if ty.uses_interface_aggregate_pointer_representation() {
            self.read_aggregate_struct_pointer_value(
                Operand::Read(Place { local: pointer }),
                &ty,
                source,
            )
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
            } else if ty.uses_interface_aggregate_pointer_representation() {
                return self.initialize_aggregate_struct_pointer(pointer, operand, &ty, provenance);
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
        } else if ty.uses_interface_aggregate_pointer_representation() {
            self.write_aggregate_struct_pointer_fields(pointer, operand, &ty, provenance)
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
        if element.underlying() == &Ty::Int(IntTy::Int) && value.ty == **element {
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            let pointer = self.new_temp(ty.clone());
            let provenance = Provenance::Source(source);
            self.emit_pointer_call(
                hir::Builtin::PointerI64New,
                Vec::new(),
                vec![Place { local: pointer }],
                provenance.clone(),
            )?;
            self.emit_pointer_call(
                hir::Builtin::PointerI64Set,
                vec![Operand::Read(Place { local: pointer }), value_operand],
                Vec::new(),
                provenance,
            )?;
            return Ok(Operand::Read(Place { local: pointer }));
        }
        if element.bootstrap_i64_struct_fields().is_none()
            && element.uses_interface_aggregate_pointer_representation()
            && value.ty == **element
        {
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            let pointer = self.new_temp(ty.clone());
            self.initialize_aggregate_struct_pointer(
                pointer,
                value_operand,
                element,
                Provenance::Source(source),
            )?;
            return Ok(Operand::Read(Place { local: pointer }));
        }
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
        if ty.bootstrap_i64_struct_fields().is_none()
            && !ty.uses_interface_aggregate_pointer_representation()
        {
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
        if ty.bootstrap_i64_struct_fields().is_some() {
            self.read_struct_pointer_value(pointer_operand, ty, source)
        } else {
            self.read_aggregate_struct_pointer_value(pointer_operand, ty, source)
        }
    }

    fn initialize_aggregate_struct_pointer(
        &mut self,
        pointer: LocalId,
        structure: Operand,
        ty: &Ty,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let Provenance::Source(source) = provenance.clone() else {
            return Err(Diagnostic::backend(
                "synthetic aggregate pointer initialization has no source provenance",
            ));
        };
        let payload = self.snapshot_interface_aggregate_struct(structure, ty, source)?;
        let pointer_ty = self.local_ty(pointer)?.clone();
        let identity = pointer_ty.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::backend("aggregate pointer omitted its dynamic type identity")
        })?;
        self.emit_pointer_call(
            hir::Builtin::AggregatePointerNew,
            vec![
                Operand::Constant(ConstValue::String(identity), Ty::String),
                payload,
            ],
            vec![Place { local: pointer }],
            provenance,
        )
    }

    fn read_aggregate_struct_pointer_value(
        &mut self,
        pointer: Operand,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let pointer_ty = Ty::Pointer(Box::new(ty.clone()));
        let identity = pointer_ty.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::backend("aggregate pointer omitted its dynamic type identity")
        })?;
        self.unbox_aggregate_struct_with(
            hir::Builtin::AggregatePointerSnapshot,
            pointer,
            &identity,
            ty,
            source,
        )
    }

    fn write_aggregate_struct_pointer_fields(
        &mut self,
        pointer: LocalId,
        structure: Operand,
        ty: &Ty,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let Provenance::Source(source) = provenance.clone() else {
            return Err(Diagnostic::backend(
                "synthetic aggregate pointer update has no source provenance",
            ));
        };
        let fields = ty.interface_aggregate_struct_fields().ok_or_else(|| {
            Diagnostic::backend("aggregate pointer update has an unsupported struct type")
        })?;
        let replacement = self.snapshot_interface_aggregate_struct(structure, ty, source)?;
        let pointer_ty = self.local_ty(pointer)?.clone();
        let identity = pointer_ty.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::backend("aggregate pointer omitted its dynamic type identity")
        })?;
        let interface_ty = Ty::Interface(Vec::new());
        let snapshot_ty = Ty::Slice(Box::new(interface_ty.clone()));
        let current = Place {
            local: self.new_temp(snapshot_ty),
        };
        self.emit_pointer_call(
            hir::Builtin::AggregatePointerSnapshot,
            vec![
                Operand::Read(Place { local: pointer }),
                Operand::Constant(ConstValue::String(identity), Ty::String),
            ],
            vec![current],
            provenance,
        )?;
        for index in 0..fields.len() {
            let tagged = Place {
                local: self.new_temp(interface_ty.clone()),
            };
            self.emit_pointer_call(
                hir::Builtin::AggregateSliceIndexTagged,
                vec![replacement.clone(), int_constant_operand(index)],
                vec![tagged],
                Provenance::Source(source),
            )?;
            self.emit_pointer_call(
                hir::Builtin::AggregateSliceSetTagged,
                vec![
                    Operand::Read(current),
                    int_constant_operand(index),
                    Operand::Read(tagged),
                ],
                Vec::new(),
                Provenance::Source(source),
            )?;
        }
        Ok(())
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
        coercions: &[hir::ValueCoercion],
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
        if coercions.len() != component_types.len() {
            return Err(Diagnostic::backend(
                "multi-result HIR assignment coercion arity changed before MIR lowering",
            ));
        }
        let temporary_results = component_types
            .iter()
            .map(|ty| Place {
                local: self.new_temp(ty.clone()),
            })
            .collect::<Vec<_>>();
        self.lower_call_into(value, temporary_results.clone())?;
        let mut assigned_operands = Vec::with_capacity(temporary_results.len());
        for ((result, source_ty), coercion) in temporary_results
            .into_iter()
            .zip(component_types)
            .zip(coercions)
        {
            assigned_operands.push(self.lower_value_coercion(
                Operand::Read(result),
                source_ty,
                coercion,
                value.source,
            )?);
        }
        for (destination, operand) in destinations.iter().zip(assigned_operands) {
            if let hir::Place::Local(local) = destination {
                self.write_semantic_local(
                    *local,
                    operand,
                    Provenance::Source(value.source),
                    initialize,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn lower_value_coercion(
        &mut self,
        operand: Operand,
        source_ty: &Ty,
        coercion: &hir::ValueCoercion,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        match coercion {
            hir::ValueCoercion::Identity => Ok(operand),
            hir::ValueCoercion::Representation { target } => {
                let result = Place {
                    local: self.new_temp(target.clone()),
                };
                let provenance = Provenance::Source(source);
                let value = make_rvalue(
                    RvalueKind::Conversion {
                        operand,
                        from: source_ty.clone(),
                        ty: target.clone(),
                    },
                    hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            hir::ValueCoercion::Interface {
                target,
                type_identity,
            } => self.box_interface_operand(operand, source_ty, type_identity, target, source),
        }
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
