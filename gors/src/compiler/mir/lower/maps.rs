//! Explicit-order lowering for concrete map operations.

use super::super::construct::{
    assignment_binary_op, binary_effects, call_effects, make_rvalue, make_statement,
    make_terminator,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_map_lookup_into(
        &mut self,
        arguments: &[hir::Expr],
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let [map, key] = arguments else {
            return Err(Diagnostic::backend(
                "map comma-ok lookup argument arity changed before MIR lowering",
            ));
        };
        let [value_destination, present_destination] = destinations.as_slice() else {
            return Err(Diagnostic::backend(
                "map comma-ok lookup result arity changed before MIR lowering",
            ));
        };
        let (get, contains) = map_get_contains_builtins(&map.ty).ok_or_else(|| {
            Diagnostic::backend("map comma-ok lookup has no concrete runtime representation")
        })?;
        let map_operand = self.lower_expr(map)?;
        let map_operand =
            self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
        let key_operand = self.lower_expr(key)?;
        let key_operand =
            self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
        self.emit_map_call(
            get,
            vec![map_operand.clone(), key_operand.clone()],
            vec![*value_destination],
            source,
        )?;
        self.emit_map_call(
            contains,
            vec![map_operand, key_operand],
            vec![*present_destination],
            source,
        )
    }

    pub(super) fn lower_zero_value(
        &mut self,
        destination: Place,
        ty: Ty,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        if let Some(zero) = ty.zero() {
            return self.write_semantic_local(
                destination.local,
                Operand::Constant(zero, ty),
                provenance,
                true,
            );
        }
        if let Ty::Array(length, element) = ty.underlying()
            && element.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int)
        {
            let length = usize::try_from(*length)
                .map_err(|_| Diagnostic::backend("verified array length does not fit usize"))?;
            let value = make_rvalue(
                RvalueKind::ArrayLiteralI64(vec![0; length]),
                hir::Effects::default(),
                provenance.clone(),
            );
            return self.push_statement(make_statement(destination, value, provenance));
        }
        if let Some(fields) = ty.bootstrap_i64_struct_fields() {
            let values = fields
                .iter()
                .map(|field| {
                    field
                        .ty
                        .zero()
                        .map(|value| Operand::Constant(value, field.ty.clone()))
                        .ok_or_else(|| {
                            Diagnostic::backend(
                                "integer struct field has no scalar zero representation",
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let value = make_rvalue(
                RvalueKind::StructLiteral { fields: values, ty },
                hir::Effects::default(),
                provenance.clone(),
            );
            return self.push_statement(make_statement(destination, value, provenance));
        }
        let zero_builtin = if is_string_i64_map(&ty) {
            Some(hir::Builtin::MapStringI64Nil)
        } else if is_i64_go_string_map(&ty) {
            Some(hir::Builtin::MapI64GoStringNil)
        } else if let Ty::Slice(element) = ty.underlying() {
            if matches!(
                element.underlying(),
                Ty::Int(crate::compiler::types::IntTy::Int | crate::compiler::types::IntTy::Int32)
            ) || element.snapshot_function_result().is_some()
            {
                Some(hir::Builtin::SliceI64Nil)
            } else if element.underlying() == &Ty::Uint(crate::compiler::types::UintTy::Uint8) {
                Some(hir::Builtin::SliceU8Nil)
            } else if element.underlying() == &Ty::Bool {
                Some(hir::Builtin::SliceBoolNil)
            } else if element.underlying() == &Ty::String {
                Some(hir::Builtin::SliceGoStringNil)
            } else if element.uses_interface_aggregate_representation() {
                Some(hir::Builtin::AggregateSliceNil)
            } else {
                None
            }
        } else if matches!(ty.underlying(), Ty::Interface(_)) {
            Some(hir::Builtin::InterfaceNil)
        } else if matches!(ty.underlying(), Ty::Function(_)) {
            Some(hir::Builtin::FunctionNil)
        } else if is_int_pointer(&ty) {
            Some(hir::Builtin::PointerI64Nil)
        } else if ty.bootstrap_i64_struct_pointer_fields().is_some() {
            Some(hir::Builtin::PointerStructI64Nil)
        } else if matches!(
            ty.underlying(),
            Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation()
        ) {
            Some(hir::Builtin::AggregatePointerNil)
        } else {
            channel_nil_builtin(&ty)
        };
        if let Some(builtin) = zero_builtin {
            let provenance = match provenance {
                Provenance::Synthetic(
                    super::SyntheticOrigin::NamedResultInitialization
                    | super::SyntheticOrigin::PanicCleanupInitialization,
                ) => Provenance::Synthetic(super::SyntheticOrigin::ZeroValueCall),
                other => other,
            };
            return self.emit_map_call_with_provenance(
                builtin,
                Vec::new(),
                vec![destination],
                provenance,
            );
        }
        Err(Diagnostic::backend(format!(
            "type {ty:?} has no MIR zero representation"
        )))
    }

    pub(super) fn lower_map_literal(
        &mut self,
        entries: &[(hir::Expr, hir::Expr)],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let (make, set) = map_make_set_builtins(ty).ok_or_else(|| {
            Diagnostic::backend("map literal has no concrete runtime representation")
        })?;
        self.emit_map_call(make, Vec::new(), vec![result], source)?;
        for (key, value) in entries {
            let key_operand = self.lower_expr(key)?;
            let key_operand =
                self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            self.emit_map_call(
                set,
                vec![Operand::Read(result), key_operand, value_operand],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_aggregate_map_literal(
        &mut self,
        entries: &[(hir::Expr, hir::Expr)],
        type_identity: &[u8],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Ty::Map(_, value_ty) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "aggregate map literal has a non-map type",
            ));
        };
        let value_ty = value_ty.as_ref().clone();
        let mut values = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let key_operand = self.lower_expr(key)?;
            let key_operand =
                self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            values.push((key_operand, value_operand));
        }
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        self.emit_map_call(
            hir::Builtin::AggregateMapMake,
            Vec::new(),
            vec![result],
            source,
        )?;
        let interface_ty = Ty::Interface(Vec::new());
        for (key, value) in values {
            let tagged =
                self.box_interface_operand(value, &value_ty, type_identity, &interface_ty, source)?;
            self.emit_map_call(
                hir::Builtin::AggregateMapSetTagged,
                vec![Operand::Read(result), key, tagged],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_aggregate_map_index(
        &mut self,
        map: &hir::Expr,
        key: &hir::Expr,
        type_identity: &[u8],
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let map_operand = self.lower_expr(map)?;
        let map_operand =
            self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
        let key_operand = self.lower_expr(key)?;
        let key_operand =
            self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
        let present = Place {
            local: self.new_temp(Ty::Bool),
        };
        self.emit_map_call(
            hir::Builtin::AggregateMapContains,
            vec![map_operand.clone(), key_operand.clone()],
            vec![present],
            source,
        )?;

        let provenance = Provenance::Source(source);
        let matched = self.new_block(provenance.clone());
        let missing = self.new_block(provenance.clone());
        let join = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: Operand::Read(present),
                then_target: matched,
                else_target: missing,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        let result = Place {
            local: self.new_temp(result_ty.clone()),
        };
        self.current = matched;
        let tagged = Place {
            local: self.new_temp(Ty::Interface(Vec::new())),
        };
        self.emit_map_call(
            hir::Builtin::AggregateMapGetTagged,
            vec![map_operand, key_operand],
            vec![tagged],
            source,
        )?;
        let value =
            self.unbox_interface_value(Operand::Read(tagged), type_identity, result_ty, source)?;
        let value = make_rvalue(
            RvalueKind::Use(value),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = missing;
        self.lower_zero_value(result, result_ty.clone(), provenance.clone())?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance,
        ))?;
        self.current = join;
        Ok(Operand::Read(result))
    }

    /// Write a map element, evaluating the map and key operands exactly once.
    ///
    /// A compound operation first reads the current element through the same
    /// operand temporaries (a missing key reads the zero value), applies the
    /// binary operation, and then performs the single write; writing through a
    /// nil map keeps the runtime's Go assignment panic.
    pub(super) fn lower_map_assignment(
        &mut self,
        map: &hir::Expr,
        key: &hir::Expr,
        op: hir::AssignOp,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let (get, _) = map_get_contains_builtins(&map.ty).ok_or_else(|| {
            Diagnostic::backend("map assignment has no concrete runtime representation")
        })?;
        let set = map_set_builtin(&map.ty)
            .ok_or_else(|| Diagnostic::backend("map assignment has no concrete set operation"))?;
        let Ty::Map(_, element_ty) = map.ty.underlying() else {
            return Err(Diagnostic::backend(
                "map assignment reached MIR with a non-map receiver",
            ));
        };
        let element_ty = element_ty.as_ref().clone();
        let map_operand = self.lower_expr(map)?;
        let map_operand =
            self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
        let key_operand = self.lower_expr(key)?;
        let key_operand =
            self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
        let assigned = if op == hir::AssignOp::Set {
            let value_operand = self.lower_expr(value)?;
            self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?
        } else {
            let provenance = Provenance::Source(source);
            let old = Place {
                local: self.new_temp(element_ty.clone()),
            };
            self.emit_map_call(
                get,
                vec![map_operand.clone(), key_operand.clone()],
                vec![old],
                source,
            )?;
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            let result = Place {
                local: self.new_temp(element_ty.clone()),
            };
            let binary_op = assignment_binary_op(op);
            let binary = make_rvalue(
                RvalueKind::Binary {
                    op: binary_op,
                    left: Operand::Read(old),
                    right: value_operand,
                    ty: element_ty.clone(),
                },
                binary_effects(binary_op, &element_ty, &value.ty),
                provenance.clone(),
            );
            self.push_statement(make_statement(result, binary, provenance))?;
            Operand::Read(result)
        };
        self.emit_map_call(
            set,
            vec![map_operand, key_operand, assigned],
            Vec::new(),
            source,
        )
    }

    pub(super) fn emit_map_call(
        &mut self,
        builtin: hir::Builtin,
        args: Vec<Operand>,
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        self.emit_map_call_with_provenance(builtin, args, destinations, Provenance::Source(source))
    }

    fn emit_map_call_with_provenance(
        &mut self,
        builtin: hir::Builtin,
        args: Vec<Operand>,
        destinations: Vec<Place>,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(builtin),
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

pub(super) fn has_mir_zero_representation(ty: &Ty) -> bool {
    ty.zero().is_some()
        || matches!(
            ty.underlying(),
            Ty::Array(_, element)
                if element.underlying()
                    == &Ty::Int(crate::compiler::types::IntTy::Int)
        )
        || is_string_i64_map(ty)
        || is_i64_go_string_map(ty)
        || matches!(ty.underlying(), Ty::Interface(_))
        || is_int_pointer(ty)
        || ty.bootstrap_i64_struct_pointer_fields().is_some()
        || matches!(
            ty.underlying(),
            Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation()
        )
        || channel_nil_builtin(ty).is_some()
}

fn is_int_pointer(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Pointer(element)
            if element.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int)
    )
}

fn is_string_i64_map(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Map(key, value)
            if key.underlying() == &Ty::String
                && value.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int)
    )
}

fn is_i64_go_string_map(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Map(key, value)
            if key.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int)
                && value.underlying() == &Ty::String
    )
}

fn map_get_contains_builtins(ty: &Ty) -> Option<(hir::Builtin, hir::Builtin)> {
    if is_string_i64_map(ty) {
        Some((
            hir::Builtin::MapStringI64Get,
            hir::Builtin::MapStringI64Contains,
        ))
    } else if is_i64_go_string_map(ty) {
        Some((
            hir::Builtin::MapI64GoStringGet,
            hir::Builtin::MapI64GoStringContains,
        ))
    } else {
        None
    }
}

fn map_make_set_builtins(ty: &Ty) -> Option<(hir::Builtin, hir::Builtin)> {
    if is_string_i64_map(ty) {
        Some((
            hir::Builtin::MapStringI64Make,
            hir::Builtin::MapStringI64Set,
        ))
    } else if is_i64_go_string_map(ty) {
        Some((
            hir::Builtin::MapI64GoStringMake,
            hir::Builtin::MapI64GoStringSet,
        ))
    } else {
        None
    }
}

pub(super) fn map_set_builtin(ty: &Ty) -> Option<hir::Builtin> {
    map_make_set_builtins(ty).map(|(_, set)| set)
}

fn channel_nil_builtin(ty: &Ty) -> Option<hir::Builtin> {
    let Ty::Channel(_, element) = ty.underlying() else {
        return None;
    };
    match element.underlying() {
        Ty::Int(crate::compiler::types::IntTy::Int) => Some(hir::Builtin::ChannelI64Nil),
        Ty::String => Some(hir::Builtin::ChannelGoStringNil),
        Ty::Channel(_, nested)
            if nested.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int) =>
        {
            Some(hir::Builtin::ChannelGoChannelI64Nil)
        }
        _ => None,
    }
}
