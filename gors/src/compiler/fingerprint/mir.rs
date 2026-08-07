//! Canonical encoding of verified or unverified Go MIR values.

use super::Fingerprint;
use super::encoder::{
    Encoder, block_id, closure_id, const_value, def_id, hir_effects, local_id, package_id,
    qualified_def_id, signature, source_ref, ty,
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
    encoder.field(b"package-id", |encoder| {
        package_id(encoder, file.package_id)
    });
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
            encoder.field(b"recovered", |encoder| local_id(encoder, cleanup.recovered));
            encoder.field(b"actions", |encoder| {
                encoder.sequence(&cleanup.actions, encode_deferred_action);
            });
            encoder.field(b"completion", |encoder| {
                block_id(encoder, cleanup.completion);
            });
        });
    });
    encoder.field(b"source", |encoder| source_ref(encoder, function.source));
}

fn encode_deferred_action(encoder: &mut Encoder, action: &mir::DeferredAction) {
    encoder.field(b"dispatch", |encoder| block_id(encoder, action.dispatch));
    encoder.field(b"registered", |encoder| {
        local_id(encoder, action.registered);
    });
    encoder.field(b"entry", |encoder| block_id(encoder, action.entry));
    encoder.field(b"blocks", |encoder| {
        encoder.sequence(&action.blocks, |encoder, block| block_id(encoder, *block));
    });
    encoder.field(b"continuation", |encoder| {
        block_id(encoder, action.continuation);
    });
    encoder.field(b"replacement", |encoder| {
        let replacement = &action.replacement;
        encoder.field(b"target", |encoder| block_id(encoder, replacement.target));
        encoder.field(b"active", |encoder| local_id(encoder, replacement.active));
        encoder.field(b"recovered", |encoder| {
            local_id(encoder, replacement.recovered);
        });
        encoder.field(b"effects", |encoder| {
            hir_effects(encoder, replacement.effects);
        });
        encoder.field(b"provenance", |encoder| {
            encode_provenance(encoder, &replacement.provenance);
        });
    });
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
        mir::RvalueKind::SliceLiteralI64 {
            elements,
            ty: slice_ty,
        } => {
            encoder.variant(b"slice-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
                ty(encoder, slice_ty);
            });
        }
        mir::RvalueKind::SliceLiteralU8(elements) => {
            encoder.variant(b"slice-literal-u8", |encoder| encoder.blob(elements));
        }
        mir::RvalueKind::SliceLiteralBool(elements) => {
            encoder.variant(b"slice-literal-bool", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.bool(*element));
            });
        }
        mir::RvalueKind::ArrayLiteralI64(elements) => {
            encoder.variant(b"array-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
            });
        }
        mir::RvalueKind::ArrayLiteral {
            elements,
            ty: literal_ty,
        } => encoder.variant(b"array-literal", |encoder| {
            encoder.field(b"elements", |encoder| {
                encoder.sequence(elements, encode_operand);
            });
            encoder.field(b"type", |encoder| ty(encoder, literal_ty));
        }),
        mir::RvalueKind::ArrayIndexI64 { array, index } => {
            encoder.variant(b"array-index-i64", |encoder| {
                encoder.field(b"array", |encoder| encode_operand(encoder, array));
                encoder.field(b"index", |encoder| encode_operand(encoder, index));
            });
        }
        mir::RvalueKind::ArrayIndex { array, index } => {
            encoder.variant(b"array-index", |encoder| {
                encoder.field(b"array", |encoder| encode_operand(encoder, array));
                encoder.field(b"index", |encoder| encode_operand(encoder, index));
            });
        }
        mir::RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => encoder.variant(b"array-set-i64", |encoder| {
            encoder.field(b"array", |encoder| encode_operand(encoder, array));
            encoder.field(b"index", |encoder| encode_operand(encoder, index));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
        mir::RvalueKind::ArraySet {
            array,
            index,
            value,
        } => encoder.variant(b"array-set", |encoder| {
            encoder.field(b"array", |encoder| encode_operand(encoder, array));
            encoder.field(b"index", |encoder| encode_operand(encoder, index));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
        mir::RvalueKind::StructLiteral {
            fields,
            ty: literal_ty,
        } => {
            encoder.variant(b"struct-literal", |encoder| {
                encoder.field(b"fields", |encoder| {
                    encoder.sequence(fields, encode_operand);
                });
                encoder.field(b"type", |encoder| ty(encoder, literal_ty));
            });
        }
        mir::RvalueKind::StructField { structure, field } => {
            encoder.variant(b"struct-field", |encoder| {
                encoder.field(b"structure", |encoder| encode_operand(encoder, structure));
                encoder.field(b"field", |encoder| encoder.u32(*field));
            });
        }
        mir::RvalueKind::StructSet {
            structure,
            field,
            value,
        } => encoder.variant(b"struct-set", |encoder| {
            encoder.field(b"structure", |encoder| encode_operand(encoder, structure));
            encoder.field(b"field", |encoder| encoder.u32(*field));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
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
        mir::RvalueKind::Recover { state, value } => {
            encoder.variant(b"recover", |encoder| {
                encoder.field(b"state", |encoder| encode_place(encoder, *state));
                encoder.field(b"value", |encoder| encode_operand(encoder, value));
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
        mir::TerminatorKind::SpawnEmpty { target } => {
            encoder.variant(b"spawn-empty", |encoder| block_id(encoder, *target));
        }
        mir::TerminatorKind::Return(values) => encoder.variant(b"return", |encoder| {
            encoder.sequence(values, encode_operand);
        }),
        mir::TerminatorKind::Unreachable => encoder.variant(b"unreachable", |_| {}),
    }
}

fn encode_callee(encoder: &mut Encoder, callee: hir::Callee) {
    match callee {
        hir::Callee::Function(id) => {
            encoder.variant(b"function", |encoder| qualified_def_id(encoder, id));
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
                    hir::Builtin::SliceI64Nil => b"slice-i64-nil",
                    hir::Builtin::SliceI64IsNil => b"slice-i64-is-nil",
                    hir::Builtin::SliceU8Make => b"slice-u8-make",
                    hir::Builtin::SliceU8Set => b"slice-u8-set",
                    hir::Builtin::SliceU8AppendSlice => b"slice-u8-append-slice",
                    hir::Builtin::SliceU8AppendString => b"slice-u8-append-string",
                    hir::Builtin::SliceU8Copy => b"slice-u8-copy",
                    hir::Builtin::SliceU8CopyString => b"slice-u8-copy-string",
                    hir::Builtin::SliceU8Len => b"slice-u8-len",
                    hir::Builtin::SliceU8Index => b"slice-u8-index",
                    hir::Builtin::SliceU8Range => b"slice-u8-range",
                    hir::Builtin::SliceU8Nil => b"slice-u8-nil",
                    hir::Builtin::SliceU8IsNil => b"slice-u8-is-nil",
                    hir::Builtin::SliceI64Copy => b"slice-i64-copy",
                    hir::Builtin::SliceI64Clear => b"slice-i64-clear",
                    hir::Builtin::SliceBoolIndex => b"slice-bool-index",
                    hir::Builtin::SliceBoolSet => b"slice-bool-set",
                    hir::Builtin::SliceBoolNil => b"slice-bool-nil",
                    hir::Builtin::SliceBoolIsNil => b"slice-bool-is-nil",
                    hir::Builtin::AggregateSliceMake => b"aggregate-slice-make",
                    hir::Builtin::AggregateSliceNil => b"aggregate-slice-nil",
                    hir::Builtin::AggregateSliceIsNil => b"aggregate-slice-is-nil",
                    hir::Builtin::AggregateSliceLen => b"aggregate-slice-len",
                    hir::Builtin::AggregateSliceIndexTagged => b"aggregate-slice-index-tagged",
                    hir::Builtin::AggregateSliceSetTagged => b"aggregate-slice-set-tagged",
                    hir::Builtin::SnapshotFunctionSliceAppend => b"snapshot-function-slice-append",
                    hir::Builtin::SnapshotFunctionSliceCall => b"snapshot-function-slice-call",
                    hir::Builtin::StringFromRune => b"string-from-rune",
                    hir::Builtin::StringFromSliceU8 => b"string-from-slice-u8",
                    hir::Builtin::StringFromSliceRunes => b"string-from-slice-runes",
                    hir::Builtin::StringLen => b"string-len",
                    hir::Builtin::StringIndex => b"string-index",
                    hir::Builtin::StringRange => b"string-range",
                    hir::Builtin::StringRangeCount => b"string-range-count",
                    hir::Builtin::StringRangeIndexAt => b"string-range-index-at",
                    hir::Builtin::StringRangeRuneAt => b"string-range-rune-at",
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
                    hir::Builtin::AggregateMapMake => b"aggregate-map-make",
                    hir::Builtin::AggregateMapLen => b"aggregate-map-len",
                    hir::Builtin::AggregateMapGetTagged => b"aggregate-map-get-tagged",
                    hir::Builtin::AggregateMapContains => b"aggregate-map-contains",
                    hir::Builtin::AggregateMapSetTagged => b"aggregate-map-set-tagged",
                    hir::Builtin::PointerI64Nil => b"pointer-i64-nil",
                    hir::Builtin::PointerI64New => b"pointer-i64-new",
                    hir::Builtin::PointerI64Get => b"pointer-i64-get",
                    hir::Builtin::PointerI64Set => b"pointer-i64-set",
                    hir::Builtin::PointerI64IsNil => b"pointer-i64-is-nil",
                    hir::Builtin::PointerStructI64Nil => b"pointer-struct-i64-nil",
                    hir::Builtin::PointerStructI64New => b"pointer-struct-i64-new",
                    hir::Builtin::PointerStructI64Get => b"pointer-struct-i64-get",
                    hir::Builtin::PointerStructI64Set => b"pointer-struct-i64-set",
                    hir::Builtin::PointerStructI64IsNil => b"pointer-struct-i64-is-nil",
                    hir::Builtin::PointerStructI64Equal => b"pointer-struct-i64-equal",
                    hir::Builtin::AggregatePointerNil => b"aggregate-pointer-nil",
                    hir::Builtin::AggregatePointerNew => b"aggregate-pointer-new",
                    hir::Builtin::AggregatePointerSnapshot => b"aggregate-pointer-snapshot",
                    hir::Builtin::AggregatePointerIsNil => b"aggregate-pointer-is-nil",
                    hir::Builtin::InterfaceNil => b"interface-nil",
                    hir::Builtin::InterfaceBoxBool => b"interface-box-bool",
                    hir::Builtin::InterfaceBoxI64 => b"interface-box-i64",
                    hir::Builtin::InterfaceBoxF64 => b"interface-box-f64",
                    hir::Builtin::InterfaceBoxGoString => b"interface-box-go-string",
                    hir::Builtin::InterfaceBoxStructI64 => b"interface-box-struct-i64",
                    hir::Builtin::InterfaceBoxPointerStructI64 => {
                        b"interface-box-pointer-struct-i64"
                    }
                    hir::Builtin::InterfaceBoxAggregate => b"interface-box-aggregate",
                    hir::Builtin::InterfaceBoxComparableAggregate => {
                        b"interface-box-comparable-aggregate"
                    }
                    hir::Builtin::InterfaceIsNil => b"interface-is-nil",
                    hir::Builtin::InterfaceIsType => b"interface-is-type",
                    hir::Builtin::InterfaceIsRuntimeError => b"interface-is-runtime-error",
                    hir::Builtin::InterfaceEqual => b"interface-equal",
                    hir::Builtin::InterfaceAssert => b"interface-assert",
                    hir::Builtin::InterfaceSatisfies => b"interface-satisfies",
                    hir::Builtin::InterfaceSatisfiesRuntimeError => {
                        b"interface-satisfies-runtime-error"
                    }
                    hir::Builtin::InterfaceSatisfiesNonNil => b"interface-satisfies-non-nil",
                    hir::Builtin::InterfaceUnboxBool => b"interface-unbox-bool",
                    hir::Builtin::InterfaceUnboxI64 => b"interface-unbox-i64",
                    hir::Builtin::InterfaceUnboxF64 => b"interface-unbox-f64",
                    hir::Builtin::InterfaceUnboxGoString => b"interface-unbox-go-string",
                    hir::Builtin::InterfaceStructI64Get => b"interface-struct-i64-get",
                    hir::Builtin::InterfaceUnboxPointerStructI64 => {
                        b"interface-unbox-pointer-struct-i64"
                    }
                    hir::Builtin::InterfaceUnboxAggregate => b"interface-unbox-aggregate",
                    hir::Builtin::FunctionNil => b"function-nil",
                    hir::Builtin::FunctionIsNil => b"function-is-nil",
                    hir::Builtin::ChannelI64Nil => b"channel-i64-nil",
                    hir::Builtin::ChannelI64Make => b"channel-i64-make",
                    hir::Builtin::ChannelI64Len => b"channel-i64-len",
                    hir::Builtin::ChannelI64Cap => b"channel-i64-cap",
                    hir::Builtin::ChannelI64Send => b"channel-i64-send",
                    hir::Builtin::ChannelI64ReceiveValue => b"channel-i64-receive-value",
                    hir::Builtin::ChannelI64Receive => b"channel-i64-receive",
                    hir::Builtin::ChannelI64Close => b"channel-i64-close",
                    hir::Builtin::ChannelI64IsNil => b"channel-i64-is-nil",
                    hir::Builtin::ChannelI64TrySend => b"channel-i64-try-send",
                    hir::Builtin::ChannelI64TryReceive => b"channel-i64-try-receive",
                    hir::Builtin::ChannelGoStringNil => b"channel-go-string-nil",
                    hir::Builtin::ChannelGoStringMake => b"channel-go-string-make",
                    hir::Builtin::ChannelGoStringLen => b"channel-go-string-len",
                    hir::Builtin::ChannelGoStringCap => b"channel-go-string-cap",
                    hir::Builtin::ChannelGoStringSend => b"channel-go-string-send",
                    hir::Builtin::ChannelGoStringReceiveValue => b"channel-go-string-receive-value",
                    hir::Builtin::ChannelGoStringReceive => b"channel-go-string-receive",
                    hir::Builtin::ChannelGoStringClose => b"channel-go-string-close",
                    hir::Builtin::ChannelGoStringIsNil => b"channel-go-string-is-nil",
                    hir::Builtin::ChannelGoStringTrySend => b"channel-go-string-try-send",
                    hir::Builtin::ChannelGoStringTryReceive => b"channel-go-string-try-receive",
                    hir::Builtin::ChannelGoChannelI64Nil => b"channel-go-channel-i64-nil",
                    hir::Builtin::ChannelGoChannelI64Make => b"channel-go-channel-i64-make",
                    hir::Builtin::ChannelGoChannelI64Len => b"channel-go-channel-i64-len",
                    hir::Builtin::ChannelGoChannelI64Cap => b"channel-go-channel-i64-cap",
                    hir::Builtin::ChannelGoChannelI64Send => b"channel-go-channel-i64-send",
                    hir::Builtin::ChannelGoChannelI64ReceiveValue => {
                        b"channel-go-channel-i64-receive-value"
                    }
                    hir::Builtin::ChannelGoChannelI64Receive => b"channel-go-channel-i64-receive",
                    hir::Builtin::ChannelGoChannelI64Close => b"channel-go-channel-i64-close",
                    hir::Builtin::ChannelGoChannelI64IsNil => b"channel-go-channel-i64-is-nil",
                    hir::Builtin::ChannelGoChannelI64TrySend => b"channel-go-channel-i64-try-send",
                    hir::Builtin::ChannelGoChannelI64TryReceive => {
                        b"channel-go-channel-i64-try-receive"
                    }
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
