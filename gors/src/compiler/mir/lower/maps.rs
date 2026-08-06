//! Explicit-order lowering for `map[string]int` operations.

use super::super::construct::{call_effects, make_rvalue, make_statement, make_terminator};
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
        let map_operand = self.lower_expr(map)?;
        let map_operand =
            self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
        let key_operand = self.lower_expr(key)?;
        let key_operand =
            self.materialize(key_operand, key.ty.clone(), Provenance::Source(key.source))?;
        self.emit_map_call(
            hir::Builtin::MapStringI64Get,
            vec![map_operand.clone(), key_operand.clone()],
            vec![*value_destination],
            source,
        )?;
        self.emit_map_call(
            hir::Builtin::MapStringI64Contains,
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
            let length = usize::try_from(*length).map_err(|_| {
                Diagnostic::backend("verified bootstrap array length does not fit usize")
            })?;
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
        } else if matches!(ty.underlying(), Ty::Interface(_)) {
            Some(hir::Builtin::InterfaceNil)
        } else if is_int_pointer(&ty) {
            Some(hir::Builtin::PointerI64Nil)
        } else if ty.bootstrap_i64_struct_pointer_fields().is_some() {
            Some(hir::Builtin::PointerStructI64Nil)
        } else if is_int_channel(&ty) {
            Some(hir::Builtin::ChannelI64Nil)
        } else {
            None
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
        self.emit_map_call(
            hir::Builtin::MapStringI64Make,
            Vec::new(),
            vec![result],
            source,
        )?;
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
                hir::Builtin::MapStringI64Set,
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

    pub(super) fn lower_map_assignment(
        &mut self,
        map: &hir::Expr,
        key: &hir::Expr,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let map_operand = self.lower_expr(map)?;
        let map_operand =
            self.materialize(map_operand, map.ty.clone(), Provenance::Source(map.source))?;
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
            hir::Builtin::MapStringI64Set,
            vec![map_operand, key_operand, value_operand],
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
        || matches!(ty.underlying(), Ty::Interface(_))
        || is_int_pointer(ty)
        || ty.bootstrap_i64_struct_pointer_fields().is_some()
        || is_int_channel(ty)
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

fn is_int_channel(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Channel(_, element)
            if element.underlying() == &Ty::Int(crate::compiler::types::IntTy::Int)
    )
}
