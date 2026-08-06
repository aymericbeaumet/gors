//! Explicit-order MIR construction for slice and map range loops.

use super::super::construct::{
    binary_effects, call_effects, make_rvalue, make_statement, make_terminator,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::{FunctionLowerer, LoopTargets};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

#[derive(Clone, Copy)]
enum RangeKind {
    Array(u64),
    Slice,
    String,
    Map,
    Channel,
    Integer(IntegerRangeKind),
}

#[derive(Clone, Copy)]
enum IntegerRangeKind {
    Int,
    Int32,
    Uint8,
}

impl IntegerRangeKind {
    fn ty(self) -> Ty {
        match self {
            Self::Int => Ty::Int(IntTy::Int),
            Self::Int32 => Ty::Int(IntTy::Int32),
            Self::Uint8 => Ty::Uint(crate::compiler::types::UintTy::Uint8),
        }
    }
}

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_range(
        &mut self,
        label: Option<&str>,
        key: Option<hir::Place>,
        value: Option<hir::Place>,
        expression: &hir::Expr,
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if let Some(label) = label {
            self.enter_label(label, source)?;
        }
        let provenance = Provenance::Source(source);
        let range_kind = match expression.ty.underlying() {
            Ty::Array(length, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                RangeKind::Array(*length)
            }
            Ty::Slice(element)
                if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
            {
                RangeKind::Slice
            }
            Ty::String => RangeKind::String,
            Ty::Map(key, value)
                if key.underlying() == &Ty::String
                    && value.underlying() == &Ty::Int(IntTy::Int) =>
            {
                RangeKind::Map
            }
            Ty::Channel(direction, element)
                if direction.can_receive() && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                RangeKind::Channel
            }
            Ty::Int(IntTy::Int) => RangeKind::Integer(IntegerRangeKind::Int),
            Ty::Int(IntTy::Int32) => RangeKind::Integer(IntegerRangeKind::Int32),
            Ty::Uint(crate::compiler::types::UintTy::Uint8) => {
                RangeKind::Integer(IntegerRangeKind::Uint8)
            }
            ty => {
                return Err(Diagnostic::backend(format!(
                    "unsupported range type {ty:?} reached MIR lowering"
                )));
            }
        };
        let container = self.lower_expr(expression)?;
        let container = self.materialize(
            container,
            expression.ty.clone(),
            Provenance::Source(expression.source),
        )?;

        if matches!(range_kind, RangeKind::Channel) {
            return self.lower_channel_range(label, key, expression, container, body, source);
        }

        let counter_ty = match range_kind {
            RangeKind::Integer(kind) => kind.ty(),
            _ => Ty::Int(IntTy::Int),
        };

        let length = Place {
            local: self.new_temp(counter_ty.clone()),
        };
        match range_kind {
            RangeKind::Array(array_length) => {
                let array_length = i64::try_from(array_length).map_err(|_| {
                    Diagnostic::backend("verified array length does not fit Go int")
                })?;
                let value = make_rvalue(
                    RvalueKind::Use(Operand::Constant(
                        ConstValue::Int(array_length.to_string()),
                        Ty::Int(IntTy::Int),
                    )),
                    hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(length, value, provenance.clone()))?;
            }
            RangeKind::Integer(_) => {
                let value = make_rvalue(
                    RvalueKind::Use(container.clone()),
                    hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(length, value, provenance.clone()))?;
            }
            RangeKind::Slice | RangeKind::String | RangeKind::Map => {
                let after_length = self.new_block(provenance.clone());
                let builtin = match range_kind {
                    RangeKind::Slice => hir::Builtin::SliceI64Len,
                    RangeKind::String => hir::Builtin::StringRangeCount,
                    RangeKind::Map => hir::Builtin::MapStringI64Len,
                    _ => {
                        return Err(Diagnostic::backend(
                            "non-container range reached runtime length lowering",
                        ));
                    }
                };
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: hir::Callee::Builtin(builtin),
                        args: vec![container.clone()],
                        destinations: vec![length],
                        target: after_length,
                    },
                    call_effects(),
                    provenance.clone(),
                ))?;
                self.current = after_length;
            }
            RangeKind::Channel => {
                return Err(Diagnostic::backend(
                    "channel range reached counted-loop lowering",
                ));
            }
        }

        let index = Place {
            local: self.new_temp(counter_ty.clone()),
        };
        let zero = make_rvalue(
            RvalueKind::Use(Operand::Constant(
                ConstValue::Int("0".into()),
                counter_ty.clone(),
            )),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(index, zero, provenance.clone()))?;

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
        let condition = Place {
            local: self.new_temp(Ty::Bool),
        };
        let comparison = make_rvalue(
            RvalueKind::Binary {
                op: hir::BinaryOp::Less,
                left: Operand::Read(index),
                right: Operand::Read(length),
                ty: Ty::Bool,
            },
            binary_effects(hir::BinaryOp::Less, &Ty::Bool),
            provenance.clone(),
        );
        self.push_statement(make_statement(condition, comparison, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: Operand::Read(condition),
                then_target: body_target,
                else_target: exit_target,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.loops.push(LoopTargets {
            label: label.map(str::to_owned),
            break_target: exit_target,
            continue_target: post_target,
            break_used: false,
        });
        self.current = body_target;
        match range_kind {
            RangeKind::Array(_) => {
                if let Some(hir::Place::Local(local)) = key {
                    self.assign_range_local(Place { local }, Operand::Read(index), source)?;
                }
                if let Some(hir::Place::Local(local)) = value {
                    let value = make_rvalue(
                        RvalueKind::ArrayIndexI64 {
                            array: container,
                            index: Operand::Read(index),
                        },
                        hir::Effects {
                            may_panic: true,
                            ..hir::Effects::default()
                        },
                        provenance.clone(),
                    );
                    self.push_statement(make_statement(
                        Place { local },
                        value,
                        provenance.clone(),
                    ))?;
                }
            }
            RangeKind::Slice => {
                if let Some(hir::Place::Local(local)) = key {
                    self.assign_range_local(Place { local }, Operand::Read(index), source)?;
                }
                if let Some(hir::Place::Local(local)) = value {
                    let after_value = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(hir::Builtin::SliceI64Index),
                            args: vec![container, Operand::Read(index)],
                            destinations: vec![Place { local }],
                            target: after_value,
                        },
                        call_effects(),
                        provenance.clone(),
                    ))?;
                    self.current = after_value;
                }
            }
            RangeKind::String => {
                if let Some(hir::Place::Local(local)) = key {
                    let range_key = Place {
                        local: self.new_temp(Ty::Int(IntTy::Int)),
                    };
                    let after_key = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(hir::Builtin::StringRangeIndexAt),
                            args: vec![container.clone(), Operand::Read(index)],
                            destinations: vec![range_key],
                            target: after_key,
                        },
                        call_effects(),
                        provenance.clone(),
                    ))?;
                    self.current = after_key;
                    self.assign_range_local(Place { local }, Operand::Read(range_key), source)?;
                }
                if let Some(hir::Place::Local(local)) = value {
                    let after_value = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(hir::Builtin::StringRangeRuneAt),
                            args: vec![container, Operand::Read(index)],
                            destinations: vec![Place { local }],
                            target: after_value,
                        },
                        call_effects(),
                        provenance.clone(),
                    ))?;
                    self.current = after_value;
                }
            }
            RangeKind::Map => {
                let needs_key = matches!(key, Some(hir::Place::Local(_)))
                    || matches!(value, Some(hir::Place::Local(_)));
                if needs_key {
                    let map_key = Place {
                        local: self.new_temp(Ty::String),
                    };
                    let after_key = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(hir::Builtin::MapStringI64KeyAt),
                            args: vec![container.clone(), Operand::Read(index)],
                            destinations: vec![map_key],
                            target: after_key,
                        },
                        call_effects(),
                        provenance.clone(),
                    ))?;
                    self.current = after_key;
                    if let Some(hir::Place::Local(local)) = key {
                        self.assign_range_local(Place { local }, Operand::Read(map_key), source)?;
                    }
                    if let Some(hir::Place::Local(local)) = value {
                        let after_value = self.new_block(provenance.clone());
                        self.terminate(make_terminator(
                            TerminatorKind::Call {
                                callee: hir::Callee::Builtin(hir::Builtin::MapStringI64Get),
                                args: vec![container, Operand::Read(map_key)],
                                destinations: vec![Place { local }],
                                target: after_value,
                            },
                            call_effects(),
                            provenance.clone(),
                        ))?;
                        self.current = after_value;
                    }
                }
            }
            RangeKind::Integer(_) => {
                if let Some(hir::Place::Local(local)) = key {
                    self.assign_range_local(Place { local }, Operand::Read(index), source)?;
                }
            }
            RangeKind::Channel => {
                return Err(Diagnostic::backend(
                    "channel range reached counted-loop body lowering",
                ));
            }
        }
        self.lower_block(body)?;
        if !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Goto(post_target),
                hir::Effects::default(),
                provenance.clone(),
            ))?;
        }

        self.current = post_target;
        let increment = make_rvalue(
            RvalueKind::Binary {
                op: hir::BinaryOp::Add,
                left: Operand::Read(index),
                right: Operand::Constant(ConstValue::Int("1".into()), counter_ty.clone()),
                ty: counter_ty.clone(),
            },
            binary_effects(hir::BinaryOp::Add, &counter_ty),
            provenance.clone(),
        );
        self.push_statement(make_statement(index, increment, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(header),
            hir::Effects::default(),
            provenance,
        ))?;
        self.loops.pop();
        self.current = exit_target;
        Ok(())
    }

    fn lower_channel_range(
        &mut self,
        label: Option<&str>,
        key: Option<hir::Place>,
        expression: &hir::Expr,
        channel: Operand,
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let Ty::Channel(_, element) = expression.ty.underlying() else {
            return Err(Diagnostic::backend(
                "non-channel expression reached channel range lowering",
            ));
        };
        let provenance = Provenance::Source(source);
        let header = self.new_block(provenance.clone());
        let received = self.new_block(provenance.clone());
        let body_target = self.new_block(provenance.clone());
        let exit_target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Goto(header),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = header;
        let value = Place {
            local: self.new_temp(element.as_ref().clone()),
        };
        let open = Place {
            local: self.new_temp(Ty::Bool),
        };
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Receive),
                args: vec![channel],
                destinations: vec![value, open],
                target: received,
            },
            call_effects(),
            provenance.clone(),
        ))?;
        self.current = received;
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: Operand::Read(open),
                then_target: body_target,
                else_target: exit_target,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.loops.push(LoopTargets {
            label: label.map(str::to_owned),
            break_target: exit_target,
            continue_target: header,
            break_used: false,
        });
        self.current = body_target;
        if let Some(hir::Place::Local(local)) = key {
            self.assign_range_local(Place { local }, Operand::Read(value), source)?;
        }
        self.lower_block(body)?;
        if !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Goto(header),
                hir::Effects::default(),
                provenance,
            ))?;
        }
        self.loops.pop();
        self.current = exit_target;
        Ok(())
    }

    fn assign_range_local(
        &mut self,
        destination: Place,
        operand: Operand,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Use(operand),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(destination, value, provenance))
    }
}
