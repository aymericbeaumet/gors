//! Explicit-order lowering for interface dynamic values.

use super::super::construct::{call_effects, make_rvalue, make_statement, make_terminator};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use super::pointers::int_constant_operand;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

impl FunctionLowerer {
    pub(super) fn lower_interface_call_expr(
        &mut self,
        receiver: &hir::Expr,
        args: &[hir::Expr],
        candidates: &[hir::InterfaceCallCandidate],
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if candidates.is_empty() {
            return Err(Diagnostic::backend(
                "interface call reached MIR without dispatch candidates",
            ));
        }
        let receiver_operand = self.lower_expr(receiver)?;
        let receiver_operand = self.materialize(
            receiver_operand,
            receiver.ty.clone(),
            Provenance::Source(receiver.source),
        )?;
        let mut argument_operands = Vec::with_capacity(args.len());
        for argument in args {
            let operand = self.lower_expr(argument)?;
            argument_operands.push(self.materialize(
                operand,
                argument.ty.clone(),
                Provenance::Source(argument.source),
            )?);
        }
        let provenance = Provenance::Source(source);
        let join = self.new_block(provenance.clone());
        let result = (*result_ty != Ty::Unit).then(|| Place {
            local: self.new_temp(result_ty.clone()),
        });
        for (index, candidate) in candidates.iter().enumerate() {
            let last = index + 1 == candidates.len();
            let next = if last {
                None
            } else {
                let condition = Place {
                    local: self.new_temp(Ty::Bool),
                };
                self.emit_map_call(
                    hir::Builtin::InterfaceIsType,
                    vec![
                        receiver_operand.clone(),
                        type_identity_operand(&candidate.type_identity),
                    ],
                    vec![condition],
                    source,
                )?;
                let matched = self.new_block(provenance.clone());
                let next = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::SwitchBool {
                        condition: Operand::Read(condition),
                        then_target: matched,
                        else_target: next,
                    },
                    hir::Effects::default(),
                    provenance.clone(),
                ))?;
                self.current = matched;
                Some(next)
            };
            self.emit_interface_candidate_call(
                receiver_operand.clone(),
                argument_operands.clone(),
                candidate,
                result,
                join,
                source,
            )?;
            if let Some(next) = next {
                self.current = next;
            }
        }
        self.current = join;
        Ok(result.map_or(Operand::Unit, Operand::Read))
    }

    pub(super) fn lower_interface_value_expr(
        &mut self,
        value: &hir::Expr,
        type_identity: &[u8],
        interface_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if !matches!(interface_ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::backend(
                "interface value has a non-interface result type",
            ));
        }
        let value_operand = self.lower_expr(value)?;
        let value_operand = self.materialize(
            value_operand,
            value.ty.clone(),
            Provenance::Source(value.source),
        )?;
        self.box_interface_operand(
            value_operand,
            &value.ty,
            type_identity,
            interface_ty,
            source,
        )
    }

    pub(super) fn lower_interface_assertion_expr(
        &mut self,
        arguments: &[hir::Expr],
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if matches!(result_ty, Ty::Tuple(_)) {
            return Err(Diagnostic::backend(
                "comma-ok interface assertion bypassed tuple lowering",
            ));
        }
        let (interface, identity) = self.lower_interface_assertion_input(arguments)?;
        self.unbox_interface_value(interface, &identity, result_ty, source)
    }

    pub(super) fn lower_interface_assertion_into(
        &mut self,
        arguments: &[hir::Expr],
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let [value_destination, ok_destination] = destinations.as_slice() else {
            return Err(Diagnostic::backend(
                "comma-ok interface assertion result arity changed before MIR lowering",
            ));
        };
        let value_ty = self.local_ty(value_destination.local)?.clone();
        if self.local_ty(ok_destination.local)?.underlying() != &Ty::Bool {
            return Err(Diagnostic::backend(
                "comma-ok interface assertion has a non-boolean status result",
            ));
        }
        let (interface, identity) = self.lower_interface_assertion_input(arguments)?;
        let condition = Place {
            local: self.new_temp(Ty::Bool),
        };
        self.emit_map_call(
            hir::Builtin::InterfaceIsType,
            vec![interface.clone(), type_identity_operand(&identity)],
            vec![condition],
            source,
        )?;

        let provenance = Provenance::Source(source);
        let matched = self.new_block(provenance.clone());
        let mismatched = self.new_block(provenance.clone());
        let join = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: Operand::Read(condition),
                then_target: matched,
                else_target: mismatched,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = matched;
        let value = self.unbox_interface_value(interface, &identity, &value_ty, source)?;
        self.assign_interface_assertion_result(*value_destination, value, provenance.clone())?;
        self.assign_interface_assertion_result(
            *ok_destination,
            Operand::Constant(ConstValue::Bool(true), Ty::Bool),
            provenance.clone(),
        )?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = mismatched;
        self.lower_zero_value(*value_destination, value_ty, provenance.clone())?;
        self.assign_interface_assertion_result(
            *ok_destination,
            Operand::Constant(ConstValue::Bool(false), Ty::Bool),
            provenance.clone(),
        )?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance,
        ))?;
        self.current = join;
        Ok(())
    }

    fn lower_interface_assertion_input(
        &mut self,
        arguments: &[hir::Expr],
    ) -> Result<(Operand, Vec<u8>), Diagnostic> {
        let [interface, identity] = arguments else {
            return Err(Diagnostic::backend(
                "interface assertion argument arity changed before MIR lowering",
            ));
        };
        if !matches!(interface.ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::backend(
                "interface assertion has a non-interface operand",
            ));
        }
        let hir::ExprKind::Constant(ConstValue::String(identity)) = &identity.kind else {
            return Err(Diagnostic::backend(
                "interface assertion type identity is not a constant string",
            ));
        };
        let operand = self.lower_expr(interface)?;
        let operand = self.materialize(
            operand,
            interface.ty.clone(),
            Provenance::Source(interface.source),
        )?;
        Ok((operand, identity.clone()))
    }

    fn assign_interface_assertion_result(
        &mut self,
        destination: Place,
        operand: Operand,
        provenance: Provenance,
    ) -> Result<(), Diagnostic> {
        let value = make_rvalue(
            RvalueKind::Use(operand),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(destination, value, provenance))
    }

    pub(super) fn unbox_interface_value(
        &mut self,
        interface: Operand,
        type_identity: &[u8],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let identity = type_identity_operand(type_identity);
        match ty.underlying() {
            Ty::Bool => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxBool,
                interface,
                identity,
                ty.clone(),
                source,
            ),
            Ty::Int(IntTy::Int) => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxI64,
                interface,
                identity,
                ty.clone(),
                source,
            ),
            Ty::String => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxGoString,
                interface,
                identity,
                ty.clone(),
                source,
            ),
            Ty::Struct(_) if ty.bootstrap_i64_struct_fields().is_some() => {
                self.unbox_interface_struct(interface, type_identity, ty, source)
            }
            Ty::Pointer(_) if ty.bootstrap_i64_struct_pointer_fields().is_some() => self
                .unbox_interface_scalar(
                    hir::Builtin::InterfaceUnboxPointerStructI64,
                    interface,
                    identity,
                    ty.clone(),
                    source,
                ),
            _ => Err(Diagnostic::backend(format!(
                "unsupported interface assertion result type {ty:?}"
            ))),
        }
    }

    pub(super) fn box_interface_operand(
        &mut self,
        value_operand: Operand,
        value_ty: &Ty,
        type_identity: &[u8],
        interface_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if !matches!(interface_ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::backend(
                "interface boxing has a non-interface result type",
            ));
        }
        let identity = Operand::Constant(ConstValue::String(type_identity.to_vec()), Ty::String);
        let (builtin, payload) = match value_ty.underlying() {
            Ty::Bool => (hir::Builtin::InterfaceBoxBool, value_operand),
            Ty::Int(IntTy::Int) => (hir::Builtin::InterfaceBoxI64, value_operand),
            Ty::String => (hir::Builtin::InterfaceBoxGoString, value_operand),
            Ty::Struct(_) if value_ty.bootstrap_i64_struct_fields().is_some() => (
                hir::Builtin::InterfaceBoxStructI64,
                self.snapshot_interface_struct(value_operand, value_ty, source)?,
            ),
            Ty::Pointer(_) if value_ty.bootstrap_i64_struct_pointer_fields().is_some() => {
                (hir::Builtin::InterfaceBoxPointerStructI64, value_operand)
            }
            _ => {
                return Err(Diagnostic::backend(format!(
                    "unsupported interface dynamic value type {:?}",
                    value_ty
                )));
            }
        };
        let result = Place {
            local: self.new_temp(interface_ty.clone()),
        };
        self.emit_map_call(builtin, vec![identity, payload], vec![result], source)?;
        Ok(Operand::Read(result))
    }

    fn snapshot_interface_struct(
        &mut self,
        structure: Operand,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let fields = ty.bootstrap_i64_struct_fields().ok_or_else(|| {
            Diagnostic::backend("interface struct snapshot has a non-integer struct type")
        })?;
        let slice_ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
        let slice = Place {
            local: self.new_temp(slice_ty),
        };
        let no_capacity = Operand::Constant(ConstValue::Int("-1".to_owned()), Ty::Int(IntTy::Int));
        self.emit_map_call(
            hir::Builtin::SliceI64Make,
            vec![int_constant_operand(fields.len()), no_capacity],
            vec![slice],
            source,
        )?;
        for (index, field) in fields.iter().enumerate() {
            let field_value = Place {
                local: self.new_temp(field.ty.clone()),
            };
            let provenance = Provenance::Source(source);
            let read = make_rvalue(
                RvalueKind::StructField {
                    structure: structure.clone(),
                    field: u32::try_from(index).map_err(|_| {
                        Diagnostic::backend("interface struct field index exceeds u32")
                    })?,
                },
                hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                provenance.clone(),
            );
            self.push_statement(make_statement(field_value, read, provenance))?;
            self.emit_map_call(
                hir::Builtin::SliceI64Set,
                vec![
                    Operand::Read(slice),
                    int_constant_operand(index),
                    Operand::Read(field_value),
                ],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(slice))
    }

    fn emit_interface_candidate_call(
        &mut self,
        interface: Operand,
        mut args: Vec<Operand>,
        candidate: &hir::InterfaceCallCandidate,
        result: Option<Place>,
        join: crate::compiler::ids::BasicBlockId,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let receiver = self.unbox_interface_receiver(interface, candidate, source)?;
        args.insert(0, receiver);
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Function(candidate.function),
                args,
                destinations: result.into_iter().collect(),
                target: join,
            },
            call_effects(),
            Provenance::Source(source),
        ))
    }

    fn unbox_interface_receiver(
        &mut self,
        interface: Operand,
        candidate: &hir::InterfaceCallCandidate,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let unboxed = match candidate.dynamic_ty.underlying() {
            Ty::Bool => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxBool,
                interface,
                type_identity_operand(&candidate.type_identity),
                candidate.dynamic_ty.clone(),
                source,
            )?,
            Ty::Int(IntTy::Int) => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxI64,
                interface,
                type_identity_operand(&candidate.type_identity),
                candidate.dynamic_ty.clone(),
                source,
            )?,
            Ty::String => self.unbox_interface_scalar(
                hir::Builtin::InterfaceUnboxGoString,
                interface,
                type_identity_operand(&candidate.type_identity),
                candidate.dynamic_ty.clone(),
                source,
            )?,
            Ty::Struct(_) if candidate.dynamic_ty.bootstrap_i64_struct_fields().is_some() => self
                .unbox_interface_struct(
                interface,
                &candidate.type_identity,
                &candidate.dynamic_ty,
                source,
            )?,
            Ty::Pointer(_)
                if candidate
                    .dynamic_ty
                    .bootstrap_i64_struct_pointer_fields()
                    .is_some() =>
            {
                self.unbox_interface_scalar(
                    hir::Builtin::InterfaceUnboxPointerStructI64,
                    interface,
                    type_identity_operand(&candidate.type_identity),
                    candidate.dynamic_ty.clone(),
                    source,
                )?
            }
            _ => {
                return Err(Diagnostic::backend(format!(
                    "unsupported interface dispatch type {:?}",
                    candidate.dynamic_ty
                )));
            }
        };
        if candidate.dynamic_ty == candidate.receiver_ty {
            return Ok(unboxed);
        }
        if candidate
            .dynamic_ty
            .bootstrap_i64_struct_pointer_fields()
            .is_some()
            && candidate
                .receiver_ty
                .bootstrap_i64_struct_fields()
                .is_some()
        {
            return self.read_struct_pointer_value(unboxed, &candidate.receiver_ty, source);
        }
        Err(Diagnostic::backend(format!(
            "cannot adapt interface dynamic receiver {:?} to method receiver {:?}",
            candidate.dynamic_ty, candidate.receiver_ty
        )))
    }

    fn unbox_interface_scalar(
        &mut self,
        builtin: hir::Builtin,
        interface: Operand,
        identity: Operand,
        ty: Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let result = Place {
            local: self.new_temp(ty),
        };
        self.emit_map_call(builtin, vec![interface, identity], vec![result], source)?;
        Ok(Operand::Read(result))
    }

    fn unbox_interface_struct(
        &mut self,
        interface: Operand,
        type_identity: &[u8],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let fields = ty.bootstrap_i64_struct_fields().ok_or_else(|| {
            Diagnostic::backend("interface struct extraction has a non-integer struct type")
        })?;
        let mut values = Vec::with_capacity(fields.len());
        for (index, field) in fields.iter().enumerate() {
            let value = Place {
                local: self.new_temp(field.ty.clone()),
            };
            self.emit_map_call(
                hir::Builtin::InterfaceStructI64Get,
                vec![
                    interface.clone(),
                    type_identity_operand(type_identity),
                    int_constant_operand(index),
                ],
                vec![value],
                source,
            )?;
            values.push(Operand::Read(value));
        }
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::StructLiteral {
                fields: values,
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }
}

fn type_identity_operand(identity: &[u8]) -> Operand {
    Operand::Constant(ConstValue::String(identity.to_vec()), Ty::String)
}
