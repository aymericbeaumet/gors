//! Canonical encoding of verified or unverified Go MIR values.

use super::Fingerprint;
use super::encoder::{
    Encoder, block_id, closure_id, const_value, def_id, hir_effects, local_id, signature,
    source_ref, ty,
};
use crate::compiler::{hir, mir};

/// Fingerprint a complete Go MIR file, including function and block order.
#[must_use]
pub fn mir_file(file: &mir::File) -> Fingerprint {
    let mut encoder = Encoder::root(b"go-mir-file");
    encode_file(&mut encoder, file);
    encoder.finish()
}

/// Fingerprint one Go MIR function independently of sibling declarations.
#[must_use]
pub fn mir_function(function: &mir::Function) -> Fingerprint {
    let mut encoder = Encoder::root(b"go-mir-function");
    encode_function(&mut encoder, function);
    encoder.finish()
}

fn encode_file(encoder: &mut Encoder, file: &mir::File) {
    encoder.field(b"package", |encoder| encoder.string(&file.package));
    encoder.field(b"functions", |encoder| {
        encoder.sequence(&file.functions, encode_function);
    });
}

fn encode_function(encoder: &mut Encoder, function: &mir::Function) {
    encoder.field(b"id", |encoder| def_id(encoder, function.id));
    encoder.field(b"name", |encoder| encoder.string(&function.name));
    encoder.field(b"signature", |encoder| {
        signature(encoder, &function.signature);
    });
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&function.params, |encoder, id| local_id(encoder, *id));
    });
    encoder.field(b"locals", |encoder| {
        encoder.sequence(&function.locals, encode_local);
    });
    encoder.field(b"blocks", |encoder| {
        encoder.sequence(&function.blocks, encode_block);
    });
    encoder.field(b"entry", |encoder| block_id(encoder, function.entry));
    encoder.field(b"panic-cleanup", |encoder| {
        encoder.option(function.panic_cleanup.as_ref(), |encoder, cleanup| {
            encoder.field(b"entry", |encoder| block_id(encoder, cleanup.entry));
            encoder.field(b"active", |encoder| local_id(encoder, cleanup.active));
        });
    });
    encoder.field(b"source", |encoder| source_ref(encoder, function.source));
}

fn encode_local(encoder: &mut Encoder, local: &mir::LocalDecl) {
    encoder.field(b"id", |encoder| local_id(encoder, local.id));
    encoder.field(b"name", |encoder| {
        encoder.option(local.name.as_ref(), |encoder, name| encoder.string(name));
    });
    encoder.field(b"type", |encoder| ty(encoder, &local.ty));
    encoder.field(b"kind", |encoder| encode_local_kind(encoder, local.kind));
}

fn encode_local_kind(encoder: &mut Encoder, kind: hir::LocalKind) {
    encoder.variant(
        match kind {
            hir::LocalKind::Parameter => b"parameter",
            hir::LocalKind::NamedResult => b"named-result",
            hir::LocalKind::Variable => b"variable",
            hir::LocalKind::Temporary => b"temporary",
        },
        |_| {},
    );
}

fn encode_block(encoder: &mut Encoder, block: &mir::BasicBlock) {
    encoder.field(b"id", |encoder| block_id(encoder, block.id));
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &block.provenance);
    });
    encoder.field(b"statements", |encoder| {
        encoder.sequence(&block.statements, encode_statement);
    });
    encoder.field(b"terminator", |encoder| {
        encode_terminator(encoder, &block.terminator);
    });
}

fn encode_statement(encoder: &mut Encoder, statement: &mir::Statement) {
    encoder.field(b"destination", |encoder| {
        encode_place(encoder, statement.destination);
    });
    encoder.field(b"value", |encoder| encode_rvalue(encoder, &statement.value));
    encoder.field(b"effects", |encoder| {
        hir_effects(encoder, statement.effects);
    });
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &statement.provenance);
    });
}

fn encode_place(encoder: &mut Encoder, place: mir::Place) {
    encoder.field(b"local", |encoder| local_id(encoder, place.local));
}

fn encode_rvalue(encoder: &mut Encoder, value: &mir::Rvalue) {
    encoder.field(b"kind", |encoder| encode_rvalue_kind(encoder, &value.kind));
    encoder.field(b"effects", |encoder| hir_effects(encoder, value.effects));
    encoder.field(b"panic", |encoder| encode_panic(encoder, value.panic));
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &value.provenance);
    });
}

fn encode_rvalue_kind(encoder: &mut Encoder, kind: &mir::RvalueKind) {
    match kind {
        mir::RvalueKind::Use(operand) => {
            encoder.variant(b"use", |encoder| encode_operand(encoder, operand));
        }
        mir::RvalueKind::SliceLiteralI64(elements) => {
            encoder.variant(b"slice-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
            });
        }
        mir::RvalueKind::SliceLiteralU8(elements) => {
            encoder.variant(b"slice-literal-u8", |encoder| encoder.blob(elements));
        }
        mir::RvalueKind::Unary {
            op,
            operand,
            ty: value_ty,
        } => {
            encoder.variant(b"unary", |encoder| {
                encoder.field(b"operation", |encoder| encode_unary_op(encoder, *op));
                encoder.field(b"operand", |encoder| encode_operand(encoder, operand));
                encoder.field(b"type", |encoder| ty(encoder, value_ty));
            });
        }
        mir::RvalueKind::Conversion {
            operand,
            from,
            ty: to,
        } => {
            encoder.variant(b"conversion", |encoder| {
                encoder.field(b"operand", |encoder| encode_operand(encoder, operand));
                encoder.field(b"from", |encoder| ty(encoder, from));
                encoder.field(b"to", |encoder| ty(encoder, to));
            });
        }
        mir::RvalueKind::RecoverCompareNil { state, equal } => {
            encoder.variant(b"recover-compare-nil", |encoder| {
                encoder.field(b"state", |encoder| encode_place(encoder, *state));
                encoder.field(b"equal", |encoder| encoder.bool(*equal));
            });
        }
        mir::RvalueKind::Binary {
            op,
            left,
            right,
            ty: value_ty,
        } => encoder.variant(b"binary", |encoder| {
            encoder.field(b"operation", |encoder| encode_binary_op(encoder, *op));
            encoder.field(b"left", |encoder| encode_operand(encoder, left));
            encoder.field(b"right", |encoder| encode_operand(encoder, right));
            encoder.field(b"type", |encoder| ty(encoder, value_ty));
        }),
    }
}

fn encode_operand(encoder: &mut Encoder, operand: &mir::Operand) {
    match operand {
        mir::Operand::Read(place) => {
            encoder.variant(b"read", |encoder| encode_place(encoder, *place));
        }
        mir::Operand::Constant(value, value_ty) => {
            encoder.variant(b"constant", |encoder| {
                encoder.field(b"value", |encoder| const_value(encoder, value));
                encoder.field(b"type", |encoder| ty(encoder, value_ty));
            });
        }
        mir::Operand::Unit => encoder.variant(b"unit", |_| {}),
    }
}

fn encode_terminator(encoder: &mut Encoder, terminator: &mir::Terminator) {
    encoder.field(b"kind", |encoder| {
        encode_terminator_kind(encoder, &terminator.kind);
    });
    encoder.field(b"effects", |encoder| {
        hir_effects(encoder, terminator.effects);
    });
    encoder.field(b"panic", |encoder| {
        encode_panic(encoder, terminator.panic);
    });
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &terminator.provenance);
    });
}

fn encode_terminator_kind(encoder: &mut Encoder, kind: &mir::TerminatorKind) {
    match kind {
        mir::TerminatorKind::Goto(target) => {
            encoder.variant(b"goto", |encoder| block_id(encoder, *target));
        }
        mir::TerminatorKind::SwitchBool {
            condition,
            then_target,
            else_target,
        } => encoder.variant(b"switch-bool", |encoder| {
            encoder.field(b"condition", |encoder| encode_operand(encoder, condition));
            encoder.field(b"then", |encoder| block_id(encoder, *then_target));
            encoder.field(b"else", |encoder| block_id(encoder, *else_target));
        }),
        mir::TerminatorKind::Call {
            callee,
            args,
            destinations,
            target,
        } => encoder.variant(b"call", |encoder| {
            encoder.field(b"callee", |encoder| encode_callee(encoder, *callee));
            encoder.field(b"arguments", |encoder| {
                encoder.sequence(args, encode_operand);
            });
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"target", |encoder| block_id(encoder, *target));
        }),
        mir::TerminatorKind::Return(values) => encoder.variant(b"return", |encoder| {
            encoder.sequence(values, encode_operand);
        }),
        mir::TerminatorKind::Unreachable => encoder.variant(b"unreachable", |_| {}),
    }
}

fn encode_callee(encoder: &mut Encoder, callee: hir::Callee) {
    match callee {
        hir::Callee::Function(id) => {
            encoder.variant(b"function", |encoder| def_id(encoder, id));
        }
        hir::Callee::Closure(id) => {
            encoder.variant(b"closure", |encoder| closure_id(encoder, id));
        }
        hir::Callee::Builtin(builtin) => encoder.variant(b"builtin", |encoder| {
            encoder.variant(
                match builtin {
                    hir::Builtin::Print => b"print",
                    hir::Builtin::Println => b"println",
                    hir::Builtin::Panic => b"panic",
                    hir::Builtin::SliceI64Index => b"slice-i64-index",
                    hir::Builtin::SliceI64Range => b"slice-i64-range",
                    hir::Builtin::SliceI64Set => b"slice-i64-set",
                    hir::Builtin::SliceI64Make => b"slice-i64-make",
                    hir::Builtin::SliceI64Len => b"slice-i64-len",
                    hir::Builtin::SliceI64Cap => b"slice-i64-cap",
                    hir::Builtin::SliceI64Append => b"slice-i64-append",
                    hir::Builtin::SliceU8AppendSlice => b"slice-u8-append-slice",
                    hir::Builtin::SliceU8AppendString => b"slice-u8-append-string",
                    hir::Builtin::SliceU8CopyString => b"slice-u8-copy-string",
                    hir::Builtin::SliceI64Copy => b"slice-i64-copy",
                    hir::Builtin::SliceI64Clear => b"slice-i64-clear",
                    hir::Builtin::StringFromSliceU8 => b"string-from-slice-u8",
                    hir::Builtin::MapStringI64Nil => b"map-string-i64-nil",
                    hir::Builtin::MapStringI64Make => b"map-string-i64-make",
                    hir::Builtin::MapStringI64Len => b"map-string-i64-len",
                    hir::Builtin::MapStringI64Get => b"map-string-i64-get",
                    hir::Builtin::MapStringI64Lookup => b"map-string-i64-lookup",
                    hir::Builtin::MapStringI64Contains => b"map-string-i64-contains",
                    hir::Builtin::MapStringI64Set => b"map-string-i64-set",
                    hir::Builtin::MapStringI64Delete => b"map-string-i64-delete",
                    hir::Builtin::MapStringI64Clear => b"map-string-i64-clear",
                    hir::Builtin::MapStringI64IsNil => b"map-string-i64-is-nil",
                    hir::Builtin::MapStringI64KeyAt => b"map-string-i64-key-at",
                },
                |_| {},
            );
        }),
    }
}

fn encode_unary_op(encoder: &mut Encoder, op: hir::UnaryOp) {
    encoder.variant(
        match op {
            hir::UnaryOp::Positive => b"positive",
            hir::UnaryOp::Negative => b"negative",
            hir::UnaryOp::Not => b"not",
            hir::UnaryOp::BitNot => b"bit-not",
            hir::UnaryOp::Real => b"real",
            hir::UnaryOp::Imag => b"imaginary",
        },
        |_| {},
    );
}

fn encode_binary_op(encoder: &mut Encoder, op: hir::BinaryOp) {
    encoder.variant(
        match op {
            hir::BinaryOp::Add => b"add",
            hir::BinaryOp::Sub => b"subtract",
            hir::BinaryOp::Mul => b"multiply",
            hir::BinaryOp::Div => b"divide",
            hir::BinaryOp::Rem => b"remainder",
            hir::BinaryOp::BitAnd => b"bit-and",
            hir::BinaryOp::BitOr => b"bit-or",
            hir::BinaryOp::BitXor => b"bit-xor",
            hir::BinaryOp::Shl => b"shift-left",
            hir::BinaryOp::Shr => b"shift-right",
            hir::BinaryOp::AndNot => b"and-not",
            hir::BinaryOp::Equal => b"equal",
            hir::BinaryOp::NotEqual => b"not-equal",
            hir::BinaryOp::Less => b"less",
            hir::BinaryOp::LessEqual => b"less-equal",
            hir::BinaryOp::Greater => b"greater",
            hir::BinaryOp::GreaterEqual => b"greater-equal",
            hir::BinaryOp::LogicalAnd => b"logical-and",
            hir::BinaryOp::LogicalOr => b"logical-or",
            hir::BinaryOp::Min => b"minimum",
            hir::BinaryOp::Max => b"maximum",
            hir::BinaryOp::Complex => b"complex",
        },
        |_| {},
    );
}

fn encode_panic(encoder: &mut Encoder, panic: mir::PanicEdge) {
    match panic {
        mir::PanicEdge::None => encoder.variant(b"none", |_| {}),
        mir::PanicEdge::Propagate => encoder.variant(b"propagate", |_| {}),
        mir::PanicEdge::Cleanup(target) => {
            encoder.variant(b"cleanup", |encoder| block_id(encoder, target));
        }
    }
}

fn encode_provenance(encoder: &mut Encoder, provenance: &mir::Provenance) {
    match provenance {
        mir::Provenance::Source(source) => {
            encoder.variant(b"source", |encoder| source_ref(encoder, *source));
        }
        mir::Provenance::Synthetic(origin) => {
            encoder.variant(b"synthetic", |encoder| {
                encoder.variant(
                    match origin {
                        mir::SyntheticOrigin::NamedResultInitialization => {
                            b"named-result-initialization"
                        }
                        mir::SyntheticOrigin::PanicCleanupInitialization => {
                            b"panic-cleanup-initialization"
                        }
                        mir::SyntheticOrigin::ZeroValueCall => b"zero-value-call",
                        mir::SyntheticOrigin::PanicCleanupDispatch => b"panic-cleanup-dispatch",
                        mir::SyntheticOrigin::ImplicitReturn => b"implicit-return",
                    },
                    |_| {},
                );
            });
        }
    }
}
