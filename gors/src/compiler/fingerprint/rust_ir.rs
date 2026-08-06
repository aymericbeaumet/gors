//! Canonical encoding of explicit Rust representation IR.

use super::Fingerprint;
use super::encoder::{
    Encoder, block_id, def_id, local_id, package_id, qualified_def_id, source_ref,
};
use crate::compiler::rust_ir;
use gors_runtime_abi::{PrimitiveOp, RuntimeOp, RuntimeRequirement};

/// Fingerprint a complete Rust IR file, including artifact publication order.
#[must_use]
pub fn rust_ir_file(file: &rust_ir::File) -> Fingerprint {
    let mut encoder = Encoder::root(b"rust-ir-file");
    encode_file(&mut encoder, file);
    encoder.finish()
}

/// Fingerprint one Rust IR function independently of sibling declarations.
#[must_use]
pub fn rust_ir_function(function: &rust_ir::Function) -> Fingerprint {
    let mut encoder = Encoder::root(b"rust-ir-function");
    encode_function(&mut encoder, function);
    encoder.finish()
}

/// Fingerprint one canonical set of runtime operations selected by verified
/// Rust representation lowering.
pub(in crate::compiler) fn runtime_requirement(requirement: &RuntimeRequirement) -> Fingerprint {
    let mut encoder = Encoder::root(b"rust-runtime-requirement");
    encoder.field(b"operations", |encoder| {
        encoder.sequence(requirement.as_slice(), |encoder, operation| {
            encode_runtime_op(encoder, *operation);
        });
    });
    encoder.finish()
}

fn encode_file(encoder: &mut Encoder, file: &rust_ir::File) {
    encoder.field(b"package-id", |encoder| {
        package_id(encoder, file.package_id)
    });
    encoder.field(b"package", |encoder| encoder.string(&file.package));
    encoder.field(b"functions", |encoder| {
        encoder.sequence(&file.functions, encode_function);
    });
}

fn encode_function(encoder: &mut Encoder, function: &rust_ir::Function) {
    encoder.field(b"id", |encoder| def_id(encoder, function.id));
    encoder.field(b"name", |encoder| encoder.string(&function.name));
    encoder.field(b"artifact", |encoder| {
        encode_artifact(encoder, &function.artifact);
    });
    encoder.field(b"signature", |encoder| {
        encode_signature(encoder, &function.signature);
    });
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&function.parameters, |encoder, id| local_id(encoder, *id));
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
    encoder.field(b"control-flow", |encoder| {
        encode_control_flow(encoder, &function.control_flow);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, function.source));
}

fn encode_artifact(encoder: &mut Encoder, artifact: &rust_ir::FunctionArtifactPlan) {
    encoder.field(b"symbol", |encoder| {
        encoder.string(&artifact.symbol.spelling);
    });
    encoder.field(b"linkage", |encoder| {
        encoder.variant(
            match artifact.linkage {
                rust_ir::RustLinkage::Internal => b"internal",
                rust_ir::RustLinkage::Public => b"public",
            },
            |_| {},
        );
    });
    encoder.field(b"entrypoint", |encoder| {
        encoder.variant(
            match artifact.entrypoint {
                rust_ir::EntrypointPlan::None => b"none",
                rust_ir::EntrypointPlan::Executable => b"executable",
            },
            |_| {},
        );
    });
}

fn encode_signature(encoder: &mut Encoder, signature: &rust_ir::Signature) {
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&signature.params, |encoder, ty| encode_type(encoder, *ty));
    });
    encoder.field(b"results", |encoder| {
        encoder.sequence(&signature.results, |encoder, ty| encode_type(encoder, *ty));
    });
}

fn encode_local(encoder: &mut Encoder, local: &rust_ir::LocalDecl) {
    encoder.field(b"id", |encoder| local_id(encoder, local.id));
    encoder.field(b"name", |encoder| {
        encoder.option(local.name.as_ref(), |encoder, name| encoder.string(name));
    });
    encoder.field(b"type", |encoder| encode_type(encoder, local.ty));
    encoder.field(b"storage", |encoder| {
        encoder.variant(
            match local.storage {
                rust_ir::StorageClass::CheckedOptionSlot => b"checked-option-slot",
            },
            |_| {},
        );
    });
    encoder.field(b"initialization", |encoder| {
        encode_initialization(encoder, local.initialization);
    });
}

fn encode_initialization(encoder: &mut Encoder, initialization: rust_ir::SlotInitialization) {
    match initialization {
        rust_ir::SlotInitialization::Parameter(index) => {
            encoder.variant(b"parameter", |encoder| encoder.usize(index));
        }
        rust_ir::SlotInitialization::Uninitialized => {
            encoder.variant(b"uninitialized", |_| {});
        }
    }
}

fn encode_control_flow(encoder: &mut Encoder, plan: &rust_ir::ControlFlowPlan) {
    match plan {
        rust_ir::ControlFlowPlan::StructuredLinear { order } => {
            encoder.variant(b"structured-linear", |encoder| {
                encoder.sequence(order, |encoder, block| block_id(encoder, *block));
            });
        }
        rust_ir::ControlFlowPlan::PcDispatchU32 => {
            encoder.variant(b"pc-dispatch-u32", |_| {});
        }
    }
}

fn encode_block(encoder: &mut Encoder, block: &rust_ir::BasicBlock) {
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

fn encode_statement(encoder: &mut Encoder, statement: &rust_ir::Statement) {
    encoder.field(b"destination", |encoder| {
        encode_place(encoder, statement.destination);
    });
    encoder.field(b"value", |encoder| encode_rvalue(encoder, &statement.value));
    encoder.field(b"store", |encoder| encode_store(encoder, statement.store));
    encoder.field(b"effects", |encoder| {
        encode_effects(encoder, statement.effects);
    });
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &statement.provenance);
    });
}

fn encode_store(encoder: &mut Encoder, store: rust_ir::StoreOp) {
    encoder.variant(
        match store {
            rust_ir::StoreOp::SetSome => b"set-some",
        },
        |_| {},
    );
}

fn encode_place(encoder: &mut Encoder, place: rust_ir::Place) {
    encoder.field(b"local", |encoder| local_id(encoder, place.local));
}

fn encode_rvalue(encoder: &mut Encoder, value: &rust_ir::Rvalue) {
    encoder.field(b"kind", |encoder| encode_rvalue_kind(encoder, &value.kind));
    encoder.field(b"effects", |encoder| encode_effects(encoder, value.effects));
    encoder.field(b"panic", |encoder| encode_panic(encoder, value.panic));
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &value.provenance);
    });
}

fn encode_rvalue_kind(encoder: &mut Encoder, kind: &rust_ir::RvalueKind) {
    match kind {
        rust_ir::RvalueKind::Use(operand) => {
            encoder.variant(b"use", |encoder| encode_operand(encoder, operand));
        }
        rust_ir::RvalueKind::Unary { op, operand } => {
            encoder.variant(b"unary", |encoder| {
                encoder.field(b"operation", |encoder| encode_value_op(encoder, *op));
                encoder.field(b"operand", |encoder| encode_operand(encoder, operand));
            });
        }
        rust_ir::RvalueKind::RecoverCompareNil { state, equal } => {
            encoder.variant(b"recover-compare-nil", |encoder| {
                encoder.field(b"state", |encoder| encode_place(encoder, *state));
                encoder.field(b"equal", |encoder| encoder.bool(*equal));
            });
        }
        rust_ir::RvalueKind::Binary { op, left, right } => {
            encoder.variant(b"binary", |encoder| {
                encoder.field(b"operation", |encoder| encode_value_op(encoder, *op));
                encoder.field(b"left", |encoder| encode_operand(encoder, left));
                encoder.field(b"right", |encoder| encode_operand(encoder, right));
            });
        }
        rust_ir::RvalueKind::ArrayIndexI64 { array, index } => {
            encoder.variant(b"array-index-i64", |encoder| {
                encoder.field(b"array", |encoder| encode_operand(encoder, array));
                encoder.field(b"index", |encoder| encode_operand(encoder, index));
            });
        }
        rust_ir::RvalueKind::ArrayIndex { array, index } => {
            encoder.variant(b"array-index", |encoder| {
                encoder.field(b"array", |encoder| encode_operand(encoder, array));
                encoder.field(b"index", |encoder| encode_operand(encoder, index));
            });
        }
        rust_ir::RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => encoder.variant(b"array-set-i64", |encoder| {
            encoder.field(b"array", |encoder| encode_operand(encoder, array));
            encoder.field(b"index", |encoder| encode_operand(encoder, index));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
        rust_ir::RvalueKind::ArraySet {
            array,
            index,
            value,
        } => encoder.variant(b"array-set", |encoder| {
            encoder.field(b"array", |encoder| encode_operand(encoder, array));
            encoder.field(b"index", |encoder| encode_operand(encoder, index));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
        rust_ir::RvalueKind::ArrayLiteral { elements, ty } => {
            encoder.variant(b"array-literal", |encoder| {
                encoder.field(b"elements", |encoder| {
                    encoder.sequence(elements, encode_operand);
                });
                encoder.field(b"type", |encoder| encode_type(encoder, *ty));
            });
        }
        rust_ir::RvalueKind::StructLiteralI64(fields) => {
            encoder.variant(b"struct-literal-i64", |encoder| {
                encoder.sequence(fields, encode_operand);
            });
        }
        rust_ir::RvalueKind::StructFieldI64 { structure, field } => {
            encoder.variant(b"struct-field-i64", |encoder| {
                encoder.field(b"structure", |encoder| encode_operand(encoder, structure));
                encoder.field(b"field", |encoder| encoder.u32(*field));
            });
        }
        rust_ir::RvalueKind::StructSetI64 {
            structure,
            field,
            value,
        } => encoder.variant(b"struct-set-i64", |encoder| {
            encoder.field(b"structure", |encoder| encode_operand(encoder, structure));
            encoder.field(b"field", |encoder| encoder.u32(*field));
            encoder.field(b"value", |encoder| encode_operand(encoder, value));
        }),
        rust_ir::RvalueKind::AggregateEqualI64 { left, right, equal } => {
            encoder.variant(b"aggregate-equal-i64", |encoder| {
                encoder.field(b"left", |encoder| encode_operand(encoder, left));
                encoder.field(b"right", |encoder| encode_operand(encoder, right));
                encoder.field(b"equal", |encoder| encoder.bool(*equal));
            });
        }
    }
}

fn encode_operand(encoder: &mut Encoder, operand: &rust_ir::Operand) {
    match operand {
        rust_ir::Operand::Read { place, op } => encoder.variant(b"read", |encoder| {
            encoder.field(b"place", |encoder| encode_place(encoder, *place));
            encoder.field(b"operation", |encoder| encode_read(encoder, *op));
        }),
        rust_ir::Operand::Constant(value) => {
            encoder.variant(b"constant", |encoder| encode_constant(encoder, value));
        }
        rust_ir::Operand::Unit => encoder.variant(b"unit", |_| {}),
    }
}

fn encode_read(encoder: &mut Encoder, read: rust_ir::ReadOp) {
    encoder.variant(
        match read {
            rust_ir::ReadOp::ProvenInitializedCopy => b"proven-initialized-copy",
            rust_ir::ReadOp::ProvenInitializedClone => b"proven-initialized-clone",
            rust_ir::ReadOp::ProvenLastUseMove => b"proven-last-use-move",
        },
        |_| {},
    );
}

fn encode_constant(encoder: &mut Encoder, constant: &rust_ir::Constant) {
    match constant {
        rust_ir::Constant::Bool(value) => {
            encoder.variant(b"bool", |encoder| encoder.bool(*value));
        }
        rust_ir::Constant::I64(value) => {
            encoder.variant(b"i64", |encoder| encoder.i64(*value));
        }
        rust_ir::Constant::F64(bits) => {
            encoder.variant(b"f64-bits", |encoder| encoder.u64(*bits));
        }
        rust_ir::Constant::Complex128 { real, imag } => {
            encoder.variant(b"complex128-bits", |encoder| {
                encoder.field(b"real", |encoder| encoder.u64(*real));
                encoder.field(b"imag", |encoder| encoder.u64(*imag));
            });
        }
        rust_ir::Constant::StaticI64Array(values) => {
            encoder.variant(b"static-i64-array", |encoder| {
                encoder.sequence(values, |encoder, value| encoder.i64(*value));
            });
        }
        rust_ir::Constant::RuntimeStaticBytes { op, bytes } => {
            encoder.variant(b"runtime-static-bytes", |encoder| {
                encoder.field(b"operation", |encoder| encode_runtime_op(encoder, *op));
                encoder.field(b"bytes", |encoder| encoder.blob(bytes));
            });
        }
        rust_ir::Constant::RuntimeStaticI64s { op, values } => {
            encoder.variant(b"runtime-static-i64s", |encoder| {
                encoder.field(b"operation", |encoder| encode_runtime_op(encoder, *op));
                encoder.field(b"values", |encoder| {
                    encoder.sequence(values, |encoder, value| encoder.i64(*value));
                });
            });
        }
        rust_ir::Constant::RuntimeStaticU8s { op, values } => {
            encoder.variant(b"runtime-static-u8s", |encoder| {
                encoder.field(b"operation", |encoder| encode_runtime_op(encoder, *op));
                encoder.field(b"values", |encoder| encoder.blob(values));
            });
        }
    }
}

fn encode_terminator(encoder: &mut Encoder, terminator: &rust_ir::Terminator) {
    encoder.field(b"kind", |encoder| {
        encode_terminator_kind(encoder, &terminator.kind);
    });
    encoder.field(b"effects", |encoder| {
        encode_effects(encoder, terminator.effects);
    });
    encoder.field(b"panic", |encoder| {
        encode_panic(encoder, terminator.panic);
    });
    encoder.field(b"provenance", |encoder| {
        encode_provenance(encoder, &terminator.provenance);
    });
}

fn encode_terminator_kind(encoder: &mut Encoder, kind: &rust_ir::TerminatorKind) {
    match kind {
        rust_ir::TerminatorKind::Goto(target) => {
            encoder.variant(b"goto", |encoder| block_id(encoder, *target));
        }
        rust_ir::TerminatorKind::SwitchBool {
            condition,
            then_target,
            else_target,
        } => encoder.variant(b"switch-bool", |encoder| {
            encoder.field(b"condition", |encoder| encode_operand(encoder, condition));
            encoder.field(b"then", |encoder| block_id(encoder, *then_target));
            encoder.field(b"else", |encoder| block_id(encoder, *else_target));
        }),
        rust_ir::TerminatorKind::Call {
            target,
            args,
            destinations,
            next,
        } => encoder.variant(b"call", |encoder| {
            encoder.field(b"target", |encoder| encode_call_target(encoder, target));
            encoder.field(b"arguments", |encoder| {
                encoder.sequence(args, encode_operand);
            });
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"next", |encoder| block_id(encoder, *next));
        }),
        rust_ir::TerminatorKind::Return(values) => encoder.variant(b"return", |encoder| {
            encoder.sequence(values, encode_operand);
        }),
        rust_ir::TerminatorKind::Unreachable => encoder.variant(b"unreachable", |_| {}),
    }
}

fn encode_call_target(encoder: &mut Encoder, target: &rust_ir::CallTarget) {
    match target {
        rust_ir::CallTarget::Function(id) => {
            encoder.variant(b"function", |encoder| qualified_def_id(encoder, *id));
        }
        rust_ir::CallTarget::Runtime(operation) => {
            encoder.variant(b"runtime", |encoder| {
                encode_runtime_op(encoder, *operation);
            });
        }
    }
}

fn encode_value_op(encoder: &mut Encoder, operation: rust_ir::ValueOp) {
    match operation {
        rust_ir::ValueOp::Primitive(operation) => encoder.variant(b"primitive", |encoder| {
            encode_primitive_op(encoder, operation);
        }),
        rust_ir::ValueOp::Runtime(operation) => encoder.variant(b"runtime", |encoder| {
            encode_runtime_op(encoder, operation);
        }),
    }
}

fn encode_primitive_op(encoder: &mut Encoder, operation: PrimitiveOp) {
    encoder.u32(u32::from(operation.id().get()));
}

fn encode_runtime_op(encoder: &mut Encoder, operation: RuntimeOp) {
    encoder.u32(u32::from(operation.id().get()));
}

fn encode_type(encoder: &mut Encoder, ty: rust_ir::RustType) {
    match ty {
        rust_ir::RustType::Unit => encoder.variant(b"unit", |_| {}),
        rust_ir::RustType::Bool => encoder.variant(b"bool", |_| {}),
        rust_ir::RustType::I64 => encoder.variant(b"i64", |_| {}),
        rust_ir::RustType::F64 => encoder.variant(b"f64", |_| {}),
        rust_ir::RustType::Complex128 => encoder.variant(b"complex128", |_| {}),
        rust_ir::RustType::GoString => encoder.variant(b"go-string", |_| {}),
        rust_ir::RustType::GoSliceI64 => encoder.variant(b"go-slice-i64", |_| {}),
        rust_ir::RustType::GoSliceU8 => encoder.variant(b"go-slice-u8", |_| {}),
        rust_ir::RustType::GoMapStringI64 => encoder.variant(b"go-map-string-i64", |_| {}),
        rust_ir::RustType::GoPointerI64 => encoder.variant(b"go-pointer-i64", |_| {}),
        rust_ir::RustType::GoPointerStructI64 => {
            encoder.variant(b"go-pointer-struct-i64", |_| {});
        }
        rust_ir::RustType::GoChannelI64 => encoder.variant(b"go-channel-i64", |_| {}),
        rust_ir::RustType::ArrayI64(length) => {
            encoder.variant(b"array-i64", |encoder| encoder.u64(length));
        }
        rust_ir::RustType::ArrayBool(length) => {
            encoder.variant(b"array-bool", |encoder| encoder.u64(length));
        }
        rust_ir::RustType::ArrayF64(length) => {
            encoder.variant(b"array-f64", |encoder| encoder.u64(length));
        }
        rust_ir::RustType::ArrayGoString(length) => {
            encoder.variant(b"array-go-string", |encoder| encoder.u64(length));
        }
        rust_ir::RustType::StructI64(length) => {
            encoder.variant(b"struct-i64", |encoder| encoder.u64(length));
        }
    }
}

fn encode_effects(encoder: &mut Encoder, effects: rust_ir::Effects) {
    encoder.field(b"may-read", |encoder| encoder.bool(effects.may_read));
    encoder.field(b"may-write", |encoder| encoder.bool(effects.may_write));
    encoder.field(b"may-call", |encoder| encoder.bool(effects.may_call));
    encoder.field(b"may-allocate", |encoder| {
        encoder.bool(effects.may_allocate)
    });
    encoder.field(b"may-block", |encoder| encoder.bool(effects.may_block));
    encoder.field(b"may-panic", |encoder| encoder.bool(effects.may_panic));
}

fn encode_panic(encoder: &mut Encoder, panic: rust_ir::PanicEdge) {
    match panic {
        rust_ir::PanicEdge::None => encoder.variant(b"none", |_| {}),
        rust_ir::PanicEdge::Propagate => encoder.variant(b"propagate", |_| {}),
        rust_ir::PanicEdge::Cleanup(target) => {
            encoder.variant(b"cleanup", |encoder| block_id(encoder, target));
        }
    }
}

fn encode_provenance(encoder: &mut Encoder, provenance: &rust_ir::Provenance) {
    match provenance {
        rust_ir::Provenance::Source(source) => {
            encoder.variant(b"source", |encoder| source_ref(encoder, *source));
        }
        rust_ir::Provenance::Synthetic(origin) => {
            encoder.variant(b"synthetic", |encoder| {
                encoder.variant(
                    match origin {
                        rust_ir::SyntheticOrigin::NamedResultInitialization => {
                            b"named-result-initialization"
                        }
                        rust_ir::SyntheticOrigin::PanicCleanupInitialization => {
                            b"panic-cleanup-initialization"
                        }
                        rust_ir::SyntheticOrigin::ZeroValueCall => b"zero-value-call",
                        rust_ir::SyntheticOrigin::PanicCleanupDispatch => b"panic-cleanup-dispatch",
                        rust_ir::SyntheticOrigin::ImplicitReturn => b"implicit-return",
                    },
                    |_| {},
                );
            });
        }
    }
}
