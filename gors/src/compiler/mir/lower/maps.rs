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
            let value = make_rvalue(
                RvalueKind::Use(Operand::Constant(zero, ty)),
                hir::Effects::default(),
                provenance.clone(),
            );
            return self.push_statement(make_statement(destination, value, provenance));
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
        let zero_builtin = if is_string_i64_map(&ty) {
            Some(hir::Builtin::MapStringI64Nil)
        } else if is_int_pointer(&ty) {
            Some(hir::Builtin::PointerI64Nil)
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
