//! One explicit prepare/read/write lifecycle for every assignment target.

use super::super::construct::{
    assignment_binary_op, binary_effects, call_effects, make_rvalue, make_statement,
    make_terminator,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use super::pointers::int_constant_operand;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{IntTy, Ty};

pub(super) struct PreparedTarget {
    kind: PreparedTargetKind,
    ty: Option<Ty>,
    source: SourceRef,
}

enum PreparedTargetKind {
    Local(LocalId),
    Discard,
    SliceIndex {
        slice: Operand,
        index: Operand,
        get: hir::Builtin,
        set: hir::Builtin,
    },
    MapIndex {
        map: Operand,
        key: Operand,
        get: hir::Builtin,
        set: hir::Builtin,
    },
    ArrayIndex {
        array: LocalId,
        index: Operand,
    },
    Pointer {
        pointer: Operand,
        get: hir::Builtin,
        set: hir::Builtin,
    },
    StructFieldPath {
        structure: LocalId,
        fields: Vec<u32>,
    },
    PointerStructField {
        pointer: Operand,
        field: u32,
        get: hir::Builtin,
        set: hir::Builtin,
    },
}

impl FunctionLowerer {
    pub(super) fn lower_target_assignment(
        &mut self,
        destinations: &[hir::AssignTarget],
        op: hir::AssignOp,
        values: &[hir::Expr],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if destinations.len() != values.len() {
            return Err(Diagnostic::backend(
                "assignment arity changed before MIR lowering",
            ));
        }
        let prepared = self.prepare_assignment_targets(destinations)?;
        if op == hir::AssignOp::Set {
            let operands = self.lower_assignment_values(values)?;
            return self.write_prepared_assignments(prepared, operands, source);
        }
        let [target] = prepared.as_slice() else {
            return Err(Diagnostic::backend(
                "compound assignment reached MIR with multiple targets",
            ));
        };
        let [value] = values else {
            return Err(Diagnostic::backend(
                "compound assignment reached MIR without one operand",
            ));
        };
        let ty = target
            .ty
            .clone()
            .ok_or_else(|| Diagnostic::backend("compound assignment reached a blank target"))?;
        // Compound assignment reads the prepared location before evaluating
        // its right-hand operand, while retaining the prepared dynamic
        // operands for the eventual single write.
        let old = self.read_prepared_assignment(target)?;
        let rhs = self.lower_expr(value)?;
        let rhs = self.materialize(rhs, value.ty.clone(), Provenance::Source(value.source))?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let binary_op = assignment_binary_op(op);
        let provenance = Provenance::Source(target.source);
        let binary = make_rvalue(
            RvalueKind::Binary {
                op: binary_op,
                left: old,
                right: rhs,
                ty: ty.clone(),
            },
            binary_effects(binary_op, &ty, &value.ty),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, binary, provenance))?;
        self.write_prepared_assignments(prepared, vec![Operand::Read(result)], source)
    }

    pub(super) fn lower_target_tuple_assignment(
        &mut self,
        destinations: &[hir::AssignTarget],
        value: &hir::Expr,
        coercions: &[hir::ValueCoercion],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::backend(
                "target tuple assignment value is not a tuple",
            ));
        };
        if destinations.len() != component_types.len() || coercions.len() != component_types.len() {
            return Err(Diagnostic::backend(
                "target tuple assignment arity changed before MIR lowering",
            ));
        }
        let prepared = self.prepare_assignment_targets(destinations)?;
        let temporary_results = component_types
            .iter()
            .map(|ty| Place {
                local: self.new_temp(ty.clone()),
            })
            .collect::<Vec<_>>();
        self.lower_call_into(value, temporary_results.clone())?;
        let mut assigned = Vec::with_capacity(temporary_results.len());
        for ((result, source_ty), coercion) in temporary_results
            .into_iter()
            .zip(component_types)
            .zip(coercions)
        {
            assigned.push(self.lower_value_coercion(
                Operand::Read(result),
                source_ty,
                coercion,
                value.source,
            )?);
        }
        self.write_prepared_assignments(prepared, assigned, source)
    }

    fn lower_assignment_values(
        &mut self,
        values: &[hir::Expr],
    ) -> Result<Vec<Operand>, Diagnostic> {
        let mut operands = Vec::with_capacity(values.len());
        for value in values {
            let operand = self.lower_expr(value)?;
            operands.push(self.materialize(
                operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?);
        }
        Ok(operands)
    }

    pub(super) fn prepare_assignment_targets(
        &mut self,
        destinations: &[hir::AssignTarget],
    ) -> Result<Vec<PreparedTarget>, Diagnostic> {
        let mut prepared = Vec::with_capacity(destinations.len());
        for destination in destinations {
            prepared.push(self.prepare_assignment_target(destination)?);
        }
        Ok(prepared)
    }

    fn prepare_assignment_target(
        &mut self,
        target: &hir::AssignTarget,
    ) -> Result<PreparedTarget, Diagnostic> {
        let kind = match &target.kind {
            hir::AssignTargetKind::Local(local) => {
                verify_target_type(target, Some(self.local_ty(*local)?))?;
                PreparedTargetKind::Local(*local)
            }
            hir::AssignTargetKind::Discard => {
                verify_target_type(target, None)?;
                PreparedTargetKind::Discard
            }
            hir::AssignTargetKind::SliceIndex { slice, index, set } => {
                let Ty::Slice(element) = slice.ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "slice assignment target has a non-slice receiver",
                    ));
                };
                verify_target_type(target, Some(element))?;
                let get = slice_get_for_set(*set).ok_or_else(|| {
                    Diagnostic::backend("slice assignment target selected an invalid set operation")
                })?;
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
                PreparedTargetKind::SliceIndex {
                    slice: slice_operand,
                    index: index_operand,
                    get,
                    set: *set,
                }
            }
            hir::AssignTargetKind::MapIndex { map, key } => {
                let Ty::Map(_, element) = map.ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "map assignment target has a non-map receiver",
                    ));
                };
                verify_target_type(target, Some(element))?;
                let (get, set) = map_operations(&map.ty).ok_or_else(|| {
                    Diagnostic::backend("map assignment target has no concrete operations")
                })?;
                let map_operand = self.lower_expr(map)?;
                let map_operand =
                    self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
                let key_operand = self.lower_expr(key)?;
                let key_operand =
                    self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
                PreparedTargetKind::MapIndex {
                    map: map_operand,
                    key: key_operand,
                    get,
                    set,
                }
            }
            hir::AssignTargetKind::ArrayIndex { array, index } => {
                let array_ty = self.local_ty(*array)?;
                let Ty::Array(_, element) = array_ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "array assignment target has a non-array local",
                    ));
                };
                verify_target_type(target, Some(element))?;
                let index_operand = self.lower_expr(index)?;
                let index_operand = self.materialize(
                    index_operand,
                    index.ty.clone(),
                    Provenance::Source(index.source),
                )?;
                PreparedTargetKind::ArrayIndex {
                    array: *array,
                    index: index_operand,
                }
            }
            hir::AssignTargetKind::Pointer { pointer, set } => {
                let Ty::Pointer(element) = pointer.ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "pointer assignment target has a non-pointer operand",
                    ));
                };
                verify_target_type(target, Some(element))?;
                let get = pointer_get_for_set(*set).ok_or_else(|| {
                    Diagnostic::backend(
                        "pointer assignment target selected an invalid set operation",
                    )
                })?;
                let pointer_operand = self.lower_expr(pointer)?;
                let pointer_operand = self.materialize(
                    pointer_operand,
                    pointer.ty.clone(),
                    Provenance::Source(pointer.source),
                )?;
                PreparedTargetKind::Pointer {
                    pointer: pointer_operand,
                    get,
                    set: *set,
                }
            }
            hir::AssignTargetKind::StructFieldPath { structure, fields } => {
                let field_ty = struct_path_type(self.local_ty(*structure)?, fields)?;
                verify_target_type(target, Some(&field_ty))?;
                PreparedTargetKind::StructFieldPath {
                    structure: *structure,
                    fields: fields.clone(),
                }
            }
            hir::AssignTargetKind::PointerStructField {
                pointer,
                field,
                set,
            } => {
                let fields = pointer
                    .ty
                    .bootstrap_i64_struct_pointer_fields()
                    .ok_or_else(|| {
                        Diagnostic::backend("pointer field target has a non-struct pointer")
                    })?;
                let field_ty = fields
                    .get(*field as usize)
                    .map(|field| &field.ty)
                    .ok_or_else(|| Diagnostic::backend("pointer field target is out of bounds"))?;
                verify_target_type(target, Some(field_ty))?;
                if *set != hir::Builtin::PointerStructI64Set {
                    return Err(Diagnostic::backend(
                        "pointer field target selected an invalid set operation",
                    ));
                }
                let pointer_operand = self.lower_expr(pointer)?;
                let pointer_operand = self.materialize(
                    pointer_operand,
                    pointer.ty.clone(),
                    Provenance::Source(pointer.source),
                )?;
                PreparedTargetKind::PointerStructField {
                    pointer: pointer_operand,
                    field: *field,
                    get: hir::Builtin::PointerStructI64Get,
                    set: *set,
                }
            }
        };
        Ok(PreparedTarget {
            kind,
            ty: target.ty.clone(),
            source: target.source,
        })
    }

    fn read_prepared_assignment(&mut self, target: &PreparedTarget) -> Result<Operand, Diagnostic> {
        let ty = target
            .ty
            .clone()
            .ok_or_else(|| Diagnostic::backend("blank assignment target cannot be read"))?;
        let provenance = Provenance::Source(target.source);
        match &target.kind {
            PreparedTargetKind::Local(local) => {
                let operand = self.read_semantic_local(*local, target.source)?;
                self.materialize(operand, ty, provenance)
            }
            PreparedTargetKind::SliceIndex {
                slice, index, get, ..
            } => self.read_runtime_target(*get, vec![slice.clone(), index.clone()], ty, provenance),
            PreparedTargetKind::MapIndex { map, key, get, .. } => {
                let destination = Place {
                    local: self.new_temp(ty),
                };
                self.emit_map_call(
                    *get,
                    vec![map.clone(), key.clone()],
                    vec![destination],
                    target.source,
                )?;
                Ok(Operand::Read(destination))
            }
            PreparedTargetKind::ArrayIndex { array, index } => {
                let array_ty = self.local_ty(*array)?.clone();
                let kind = if ty.underlying() == &Ty::Int(IntTy::Int) {
                    RvalueKind::ArrayIndexI64 {
                        array: Operand::Read(Place { local: *array }),
                        index: index.clone(),
                    }
                } else {
                    RvalueKind::ArrayIndex {
                        array: Operand::Read(Place { local: *array }),
                        index: index.clone(),
                    }
                };
                let _ = array_ty;
                self.read_rvalue_target(kind, ty, array_effects(), provenance)
            }
            PreparedTargetKind::Pointer { pointer, get, .. } => {
                self.read_runtime_target(*get, vec![pointer.clone()], ty, provenance)
            }
            PreparedTargetKind::StructFieldPath { structure, fields } => {
                self.read_struct_path(*structure, fields, target.source)
            }
            PreparedTargetKind::PointerStructField {
                pointer,
                field,
                get,
                ..
            } => self.read_runtime_target(
                *get,
                vec![pointer.clone(), int_constant_operand(*field as usize)],
                ty,
                provenance,
            ),
            PreparedTargetKind::Discard => Err(Diagnostic::backend(
                "blank assignment target cannot be read",
            )),
        }
    }

    fn read_runtime_target(
        &mut self,
        builtin: hir::Builtin,
        args: Vec<Operand>,
        ty: Ty,
        provenance: Provenance,
    ) -> Result<Operand, Diagnostic> {
        let destination = Place {
            local: self.new_temp(ty),
        };
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args,
                destinations: vec![destination],
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(Operand::Read(destination))
    }

    fn read_rvalue_target(
        &mut self,
        kind: RvalueKind,
        ty: Ty,
        effects: hir::Effects,
        provenance: Provenance,
    ) -> Result<Operand, Diagnostic> {
        let destination = Place {
            local: self.new_temp(ty),
        };
        let value = make_rvalue(kind, effects, provenance.clone());
        self.push_statement(make_statement(destination, value, provenance))?;
        Ok(Operand::Read(destination))
    }

    fn read_struct_path(
        &mut self,
        structure: LocalId,
        fields: &[u32],
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let mut ty = self.local_ty(structure)?.clone();
        let mut operand = self.read_semantic_local(structure, source)?;
        operand = self.materialize(operand, ty.clone(), Provenance::Source(source))?;
        for field in fields {
            ty = struct_field_type(&ty, *field)?;
            operand = self.read_rvalue_target(
                RvalueKind::StructField {
                    structure: operand,
                    field: *field,
                },
                ty.clone(),
                hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                Provenance::Source(source),
            )?;
        }
        Ok(operand)
    }

    pub(super) fn write_prepared_assignments(
        &mut self,
        prepared: Vec<PreparedTarget>,
        operands: Vec<Operand>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if prepared.len() != operands.len() {
            return Err(Diagnostic::backend(
                "prepared assignment arity changed before destination writes",
            ));
        }
        for (destination, operand) in prepared.into_iter().zip(operands) {
            self.write_prepared_assignment(destination, operand, source)?;
        }
        Ok(())
    }

    fn write_prepared_assignment(
        &mut self,
        target: PreparedTarget,
        operand: Operand,
        _statement_source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(target.source);
        match target.kind {
            PreparedTargetKind::Local(destination) => {
                self.write_semantic_local(destination, operand, provenance, false)
            }
            PreparedTargetKind::Discard => Ok(()),
            PreparedTargetKind::SliceIndex {
                slice, index, set, ..
            } => self.emit_target_call(set, vec![slice, index, operand], provenance),
            PreparedTargetKind::MapIndex { map, key, set, .. } => {
                self.emit_map_call(set, vec![map, key, operand], Vec::new(), target.source)
            }
            PreparedTargetKind::ArrayIndex { array, index } => {
                let array_place = Place { local: array };
                let element_ty = target.ty.ok_or_else(|| {
                    Diagnostic::backend("array assignment target lost its element type")
                })?;
                let kind = if element_ty.underlying() == &Ty::Int(IntTy::Int) {
                    RvalueKind::ArraySetI64 {
                        array: Operand::Read(array_place),
                        index,
                        value: operand,
                    }
                } else {
                    RvalueKind::ArraySet {
                        array: Operand::Read(array_place),
                        index,
                        value: operand,
                    }
                };
                let updated = make_rvalue(kind, array_effects(), provenance.clone());
                self.push_statement(make_statement(array_place, updated, provenance))
            }
            PreparedTargetKind::Pointer { pointer, set, .. } => {
                self.emit_target_call(set, vec![pointer, operand], provenance)
            }
            PreparedTargetKind::StructFieldPath { structure, fields } => {
                self.write_struct_path(structure, &fields, operand, target.source)
            }
            PreparedTargetKind::PointerStructField {
                pointer,
                field,
                set,
                ..
            } => self.emit_target_call(
                set,
                vec![pointer, int_constant_operand(field as usize), operand],
                provenance,
            ),
        }
    }

    fn emit_target_call(
        &mut self,
        builtin: hir::Builtin,
        args: Vec<Operand>,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args,
                destinations: Vec::new(),
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(())
    }

    fn write_struct_path(
        &mut self,
        structure: LocalId,
        fields: &[u32],
        value: Operand,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let Some((&leaf, parents)) = fields.split_last() else {
            return Err(Diagnostic::backend("struct assignment path is empty"));
        };
        let root_ty = self.local_ty(structure)?.clone();
        let mut parent_ty = root_ty;
        let mut parent = self.read_semantic_local(structure, source)?;
        parent = self.materialize(parent, parent_ty.clone(), Provenance::Source(source))?;
        let mut ancestry = Vec::with_capacity(parents.len());
        for field in parents {
            let child_ty = struct_field_type(&parent_ty, *field)?;
            ancestry.push((parent, parent_ty, *field));
            parent = self.read_rvalue_target(
                RvalueKind::StructField {
                    structure: ancestry
                        .last()
                        .ok_or_else(|| Diagnostic::backend("struct ancestry disappeared"))?
                        .0
                        .clone(),
                    field: *field,
                },
                child_ty.clone(),
                hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                Provenance::Source(source),
            )?;
            parent_ty = child_ty;
        }
        let mut updated = self.struct_set_operand(parent, parent_ty, leaf, value, source)?;
        for (parent, parent_ty, field) in ancestry.into_iter().rev() {
            updated = self.struct_set_operand(parent, parent_ty, field, updated, source)?;
        }
        self.write_semantic_local(structure, updated, Provenance::Source(source), false)
    }

    fn struct_set_operand(
        &mut self,
        structure: Operand,
        structure_ty: Ty,
        field: u32,
        value: Operand,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let destination = Place {
            local: self.new_temp(structure_ty),
        };
        let provenance = Provenance::Source(source);
        let updated = make_rvalue(
            RvalueKind::StructSet {
                structure,
                field,
                value,
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(destination, updated, provenance))?;
        Ok(Operand::Read(destination))
    }
}

fn verify_target_type(target: &hir::AssignTarget, expected: Option<&Ty>) -> Result<(), Diagnostic> {
    if target.ty.as_ref() != expected {
        return Err(Diagnostic::backend(format!(
            "assignment target type changed before MIR lowering: {:?} != {expected:?}",
            target.ty
        )));
    }
    Ok(())
}

fn slice_get_for_set(set: hir::Builtin) -> Option<hir::Builtin> {
    match set {
        hir::Builtin::SliceI64Set => Some(hir::Builtin::SliceI64Index),
        hir::Builtin::SliceU8Set => Some(hir::Builtin::SliceU8Index),
        hir::Builtin::SliceBoolSet => Some(hir::Builtin::SliceBoolIndex),
        hir::Builtin::SliceGoStringSet => Some(hir::Builtin::SliceGoStringIndex),
        _ => None,
    }
}

fn pointer_get_for_set(set: hir::Builtin) -> Option<hir::Builtin> {
    match set {
        hir::Builtin::PointerI64Set => Some(hir::Builtin::PointerI64Get),
        _ => None,
    }
}

fn map_operations(ty: &Ty) -> Option<(hir::Builtin, hir::Builtin)> {
    match ty.underlying() {
        Ty::Map(key, value)
            if key.underlying() == &Ty::String && value.underlying() == &Ty::Int(IntTy::Int) =>
        {
            Some((hir::Builtin::MapStringI64Get, hir::Builtin::MapStringI64Set))
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::Int(IntTy::Int) && value.underlying() == &Ty::String =>
        {
            Some((
                hir::Builtin::MapI64GoStringGet,
                hir::Builtin::MapI64GoStringSet,
            ))
        }
        _ => None,
    }
}

fn array_effects() -> hir::Effects {
    hir::Effects {
        may_panic: true,
        ..hir::Effects::default()
    }
}

fn struct_path_type(root: &Ty, fields: &[u32]) -> Result<Ty, Diagnostic> {
    if fields.is_empty() {
        return Err(Diagnostic::backend("struct assignment path is empty"));
    }
    let mut ty = root.clone();
    for field in fields {
        ty = struct_field_type(&ty, *field)?;
    }
    Ok(ty)
}

fn struct_field_type(structure: &Ty, field: u32) -> Result<Ty, Diagnostic> {
    let Ty::Struct(fields) = structure.underlying() else {
        return Err(Diagnostic::backend(
            "struct assignment path crossed a non-struct type",
        ));
    };
    fields
        .get(field as usize)
        .map(|field| field.ty.clone())
        .ok_or_else(|| Diagnostic::backend("struct assignment path is out of bounds"))
}
