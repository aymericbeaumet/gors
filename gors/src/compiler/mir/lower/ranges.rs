//! Explicit-order MIR construction for range loops.

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
    Slice {
        len: hir::Builtin,
        index: hir::Builtin,
    },
    String,
    Map(MapRangeKind),
    Channel,
    Integer(IntegerRangeKind),
}

#[derive(Clone, Copy)]
enum IntegerRangeKind {
    Int,
    Int32,
    Uint8,
}

#[derive(Clone, Copy)]
enum MapRangeKind {
    StringI64,
    I64GoString,
}

impl MapRangeKind {
    fn key_ty(self) -> Ty {
        match self {
            Self::StringI64 => Ty::String,
            Self::I64GoString => Ty::Int(IntTy::Int),
        }
    }

    fn snapshot(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64RangeKeys,
            Self::I64GoString => hir::Builtin::MapI64GoStringRangeKeys,
        }
    }

    fn snapshot_len(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::SliceGoStringLen,
            Self::I64GoString => hir::Builtin::SliceI64Len,
        }
    }

    fn snapshot_index(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::SliceGoStringIndex,
            Self::I64GoString => hir::Builtin::SliceI64Index,
        }
    }

    fn contains(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Contains,
            Self::I64GoString => hir::Builtin::MapI64GoStringContains,
        }
    }

    fn get(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Get,
            Self::I64GoString => hir::Builtin::MapI64GoStringGet,
        }
    }
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
                RangeKind::Slice {
                    len: hir::Builtin::SliceI64Len,
                    index: hir::Builtin::SliceI64Index,
                }
            }
            Ty::Slice(element) if element.underlying() == &Ty::String => RangeKind::Slice {
                len: hir::Builtin::SliceGoStringLen,
                index: hir::Builtin::SliceGoStringIndex,
            },
            Ty::String => RangeKind::String,
            Ty::Map(key, value)
                if key.underlying() == &Ty::String
                    && value.underlying() == &Ty::Int(IntTy::Int) =>
            {
                RangeKind::Map(MapRangeKind::StringI64)
            }
            Ty::Map(key, value)
                if key.underlying() == &Ty::Int(IntTy::Int)
                    && value.underlying() == &Ty::String =>
            {
                RangeKind::Map(MapRangeKind::I64GoString)
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

        let iteration_container = if let RangeKind::Map(map_kind) = range_kind {
            let snapshot = Place {
                local: self.new_temp(Ty::Slice(Box::new(map_kind.key_ty()))),
            };
            let after_snapshot = self.new_block(provenance.clone());
            self.terminate(make_terminator(
                TerminatorKind::Call {
                    callee: hir::Callee::Builtin(map_kind.snapshot()),
                    args: vec![container.clone()],
                    destinations: vec![snapshot],
                    target: after_snapshot,
                },
                call_effects(),
                provenance.clone(),
            ))?;
            self.current = after_snapshot;
            Operand::Read(snapshot)
        } else {
            container.clone()
        };

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
            RangeKind::Slice { .. } | RangeKind::String | RangeKind::Map(_) => {
                let after_length = self.new_block(provenance.clone());
                let builtin = match range_kind {
                    RangeKind::Slice { len, .. } => len,
                    RangeKind::String => hir::Builtin::StringRangeCount,
                    RangeKind::Map(map_kind) => map_kind.snapshot_len(),
                    _ => {
                        return Err(Diagnostic::backend(
                            "non-container range reached runtime length lowering",
                        ));
                    }
                };
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: hir::Callee::Builtin(builtin),
                        args: vec![iteration_container.clone()],
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
            binary_effects(hir::BinaryOp::Less, &Ty::Bool, &counter_ty),
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
            RangeKind::Slice { index: builtin, .. } => {
                if let Some(hir::Place::Local(local)) = key {
                    self.assign_range_local(Place { local }, Operand::Read(index), source)?;
                }
                if let Some(hir::Place::Local(local)) = value {
                    let after_value = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(builtin),
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
            RangeKind::Map(map_kind) => {
                let map_key = Place {
                    local: self.new_temp(map_kind.key_ty()),
                };
                let after_key = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: hir::Callee::Builtin(map_kind.snapshot_index()),
                        args: vec![iteration_container, Operand::Read(index)],
                        destinations: vec![map_key],
                        target: after_key,
                    },
                    call_effects(),
                    provenance.clone(),
                ))?;
                self.current = after_key;

                let present = Place {
                    local: self.new_temp(Ty::Bool),
                };
                let after_contains = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::Call {
                        callee: hir::Callee::Builtin(map_kind.contains()),
                        args: vec![container.clone(), Operand::Read(map_key)],
                        destinations: vec![present],
                        target: after_contains,
                    },
                    call_effects(),
                    provenance.clone(),
                ))?;
                self.current = after_contains;
                let present_target = self.new_block(provenance.clone());
                self.terminate(make_terminator(
                    TerminatorKind::SwitchBool {
                        condition: Operand::Read(present),
                        then_target: present_target,
                        else_target: post_target,
                    },
                    hir::Effects::default(),
                    provenance.clone(),
                ))?;
                self.current = present_target;
                if let Some(hir::Place::Local(local)) = key {
                    self.assign_range_local(Place { local }, Operand::Read(map_key), source)?;
                }
                if let Some(hir::Place::Local(local)) = value {
                    let after_value = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(map_kind.get()),
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
            binary_effects(hir::BinaryOp::Add, &counter_ty, &counter_ty),
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
        let receive_builtin = channel_receive_builtin(element).ok_or_else(|| {
            Diagnostic::backend("unsupported channel element reached range lowering")
        })?;
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
                callee: hir::Callee::Builtin(receive_builtin),
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

fn channel_receive_builtin(element: &Ty) -> Option<hir::Builtin> {
    match element.underlying() {
        Ty::Int(IntTy::Int) => Some(hir::Builtin::ChannelI64Receive),
        Ty::String => Some(hir::Builtin::ChannelGoStringReceive),
        Ty::Channel(_, nested) if nested.underlying() == &Ty::Int(IntTy::Int) => {
            Some(hir::Builtin::ChannelGoChannelI64Receive)
        }
        _ => None,
    }
}
