//! Mandatory conversion from normalized Go MIR to explicit Rust IR.

mod aggregates;
mod control;
mod numeric;
mod printing;
mod provenance;
mod recovery;

use super::type_lowering::{integer_kind, lower_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use crate::compiler::types::{FloatTy, Signature as GoSignature, Ty};
use gors_runtime_abi::{IntegerKind, PrimitiveOp, RuntimeOp};

use control::{finish_terminator, lower_panic_edge};
use numeric::{lower_binary_op, lower_constant, lower_unary_op};
use printing::lower_print_call;
use provenance::lower_provenance;

#[cfg(test)]
pub(super) fn lower_file(file: mir::File) -> Result<out::File, Diagnostic> {
    let executable_package = file.package == "main";
    Ok(out::File {
        package_id: file.package_id,
        package: file.package,
        functions: file
            .functions
            .into_iter()
            .map(|function| lower_function(function, executable_package))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) fn lower_function(
    function: mir::Function,
    executable_package: bool,
) -> Result<out::Function, Diagnostic> {
    let artifact = if executable_package && function.name == "main" {
        out::FunctionArtifactPlan::executable_entrypoint()
    } else {
        out::FunctionArtifactPlan::public_definition(function.id)
    };
    let signature = lower_signature(&function.signature)?;
    let mut panic_cleanup = function.panic_cleanup.map(recovery::lower_panic_cleanup);
    let locals = function
        .locals
        .into_iter()
        .map(|local| {
            let initialization = function
                .params
                .iter()
                .position(|parameter| *parameter == local.id)
                .map_or(
                    out::SlotInitialization::Uninitialized,
                    out::SlotInitialization::Parameter,
                );
            Ok(out::LocalDecl {
                id: local.id,
                name: local.name,
                ty: lower_type(&local.ty)?,
                storage: out::StorageClass::CheckedOptionSlot,
                initialization,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let original_block_count = function.blocks.len();
    let mut extra_blocks = Vec::new();
    let mut blocks = function
        .blocks
        .into_iter()
        .map(|block| lower_block(block, &locals, original_block_count, &mut extra_blocks))
        .collect::<Result<Vec<_>, _>>()?;
    blocks.append(&mut extra_blocks);
    recovery::complete_action_blocks(panic_cleanup.as_mut(), &blocks)?;
    let mut lowered = out::Function {
        id: function.id,
        name: function.name,
        artifact,
        signature,
        parameters: function.params,
        locals,
        blocks,
        entry: function.entry,
        panic_cleanup,
        control_flow: out::ControlFlowPlan::PcDispatchU32,
        source: function.source,
    };
    out::select_read_operations(&mut lowered)?;
    out::select_control_flow_plan(&mut lowered)?;
    Ok(lowered)
}

pub(super) fn lower_signature(signature: &GoSignature) -> Result<out::Signature, Diagnostic> {
    Ok(out::Signature {
        params: signature
            .params
            .iter()
            .map(lower_type)
            .collect::<Result<Vec<_>, _>>()?,
        results: signature
            .results
            .iter()
            .map(lower_type)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn lower_block(
    block: mir::BasicBlock,
    locals: &[out::LocalDecl],
    original_block_count: usize,
    extra_blocks: &mut Vec<out::BasicBlock>,
) -> Result<out::BasicBlock, Diagnostic> {
    Ok(out::BasicBlock {
        id: block.id,
        provenance: lower_provenance(block.provenance),
        statements: block
            .statements
            .into_iter()
            .map(|statement| lower_statement(statement, locals))
            .collect::<Result<Vec<_>, _>>()?,
        terminator: lower_terminator(block.terminator, locals, original_block_count, extra_blocks)?,
    })
}

fn lower_statement(
    statement: mir::Statement,
    locals: &[out::LocalDecl],
) -> Result<out::Statement, Diagnostic> {
    let value = lower_rvalue(statement.value, locals)?;
    Ok(out::Statement {
        destination: lower_place(statement.destination),
        effects: out::statement_effects(&value),
        value,
        store: out::StoreOp::SetSome,
        provenance: lower_provenance(statement.provenance),
    })
}

fn lower_rvalue(rvalue: mir::Rvalue, locals: &[out::LocalDecl]) -> Result<out::Rvalue, Diagnostic> {
    let panic = rvalue.panic;
    let kind = match rvalue.kind {
        mir::RvalueKind::Use(operand) => out::RvalueKind::Use(lower_operand(operand, locals)?),
        mir::RvalueKind::SliceLiteralI64 { elements, .. } => {
            out::RvalueKind::Use(out::Operand::Constant(out::Constant::RuntimeStaticI64s {
                op: RuntimeOp::GoSliceI64FromStatic,
                values: elements,
            }))
        }
        mir::RvalueKind::SliceLiteralU8(elements) => {
            out::RvalueKind::Use(out::Operand::Constant(out::Constant::RuntimeStaticU8s {
                op: RuntimeOp::GoSliceU8FromStatic,
                values: elements,
            }))
        }
        mir::RvalueKind::SliceLiteralBool(elements) => {
            out::RvalueKind::Use(out::Operand::Constant(out::Constant::RuntimeStaticBools {
                op: RuntimeOp::GoSliceBoolFromStatic,
                values: elements,
            }))
        }
        mir::RvalueKind::ArrayLiteralI64(elements) => {
            out::RvalueKind::Use(out::Operand::Constant(out::Constant::StaticIntegerArray {
                kind: IntegerKind::I64,
                values: elements,
            }))
        }
        mir::RvalueKind::ArrayLiteral { elements, ty } => {
            let ty = lower_type(&ty)?;
            if ty.scalar_array_parts().is_none()
                && !(ty == out::RustType::ZeroArray && elements.is_empty())
            {
                return Err(Diagnostic::backend(
                    "non-scalar array reached scalar array representation lowering",
                ));
            }
            out::RvalueKind::ArrayLiteral {
                elements: elements
                    .into_iter()
                    .map(|element| lower_operand(element, locals))
                    .collect::<Result<Vec<_>, _>>()?,
                ty,
            }
        }
        mir::RvalueKind::ArrayIndexI64 { array, index } => out::RvalueKind::ArrayIndexI64 {
            array: lower_operand(array, locals)?,
            index: lower_operand(index, locals)?,
        },
        mir::RvalueKind::ArrayIndex { array, index } => out::RvalueKind::ArrayIndex {
            array: lower_operand(array, locals)?,
            index: lower_operand(index, locals)?,
        },
        mir::RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => out::RvalueKind::ArraySetI64 {
            array: lower_operand(array, locals)?,
            index: lower_operand(index, locals)?,
            value: lower_operand(value, locals)?,
        },
        mir::RvalueKind::ArraySet {
            array,
            index,
            value,
        } => out::RvalueKind::ArraySet {
            array: lower_operand(array, locals)?,
            index: lower_operand(index, locals)?,
            value: lower_operand(value, locals)?,
        },
        mir::RvalueKind::StructLiteral { fields, ty } => {
            aggregates::lower_struct_literal(fields, ty, locals)?
        }
        mir::RvalueKind::StructField { structure, field } => {
            aggregates::lower_struct_field(structure, field, locals)?
        }
        mir::RvalueKind::StructSet {
            structure,
            field,
            value,
        } => aggregates::lower_struct_set(structure, field, value, locals)?,
        mir::RvalueKind::Unary { op, operand, ty } => {
            let operand_ty = mir_operand_type(&operand, locals)?;
            let operand = lower_operand(operand, locals)?;
            match lower_unary_op(op, &ty, operand_ty, lower_type(&ty)?)? {
                Some(op) => out::RvalueKind::Unary { op, operand },
                None => out::RvalueKind::Use(operand),
            }
        }
        mir::RvalueKind::Conversion { operand, from, ty } => {
            let operation = match (from.underlying(), ty.underlying()) {
                (Ty::Int(_) | Ty::Uint(_), Ty::Int(_) | Ty::Uint(_)) => {
                    Some(out::ValueOp::Primitive(PrimitiveOp::IntegerConvert {
                        from: integer_kind(&from).ok_or_else(|| {
                            Diagnostic::backend("missing source integer representation")
                        })?,
                        to: integer_kind(&ty).ok_or_else(|| {
                            Diagnostic::backend("missing destination integer representation")
                        })?,
                    }))
                }
                (Ty::Float(_), Ty::Float(FloatTy::Float32)) => {
                    Some(out::ValueOp::Primitive(PrimitiveOp::FloatRound32))
                }
                _ => None,
            };
            let from_representation = lower_type(&from)?;
            let to_representation = lower_type(&ty)?;
            if from_representation != to_representation && operation.is_none() {
                return Err(Diagnostic::backend(format!(
                    "representation-preserving conversion changed Rust type from {from_representation:?} to {to_representation:?}"
                )));
            }
            let operand = lower_operand(operand, locals)?;
            if let Some(op) = operation {
                out::RvalueKind::Unary { op, operand }
            } else {
                out::RvalueKind::Use(operand)
            }
        }
        mir::RvalueKind::Recover { state, value } => out::RvalueKind::Recover {
            state: lower_place(state),
            value: lower_operand(value, locals)?,
            nil: RuntimeOp::GoInterfaceNil,
        },
        mir::RvalueKind::Binary {
            op,
            left,
            right,
            ty,
        } => {
            let left_ty = mir_operand_type(&left, locals)?;
            let right_ty = mir_operand_type(&right, locals)?;
            let result_ty = lower_type(&ty)?;
            if matches!(op, hir::BinaryOp::Equal | hir::BinaryOp::NotEqual)
                && left_ty == right_ty
                && matches!(
                    left_ty,
                    out::RustType::ArrayInteger { .. }
                        | out::RustType::ZeroArray
                        | out::RustType::StructI64(_)
                )
                && result_ty == out::RustType::Bool
            {
                out::RvalueKind::AggregateEqualInteger {
                    left: lower_operand(left, locals)?,
                    right: lower_operand(right, locals)?,
                    equal: op == hir::BinaryOp::Equal,
                }
            } else {
                out::RvalueKind::Binary {
                    op: lower_binary_op(op, &ty, left_ty, right_ty, result_ty)?,
                    left: lower_operand(left, locals)?,
                    right: lower_operand(right, locals)?,
                }
            }
        }
    };
    let effects = out::rvalue_effects(&kind);
    Ok(out::Rvalue {
        kind,
        effects,
        panic: lower_panic_edge(panic, effects),
        provenance: lower_provenance(rvalue.provenance),
    })
}

fn lower_terminator(
    terminator: mir::Terminator,
    locals: &[out::LocalDecl],
    original_block_count: usize,
    extra_blocks: &mut Vec<out::BasicBlock>,
) -> Result<out::Terminator, Diagnostic> {
    let panic = terminator.panic;
    let provenance = lower_provenance(terminator.provenance);
    let kind = match terminator.kind {
        mir::TerminatorKind::Goto(target) => out::TerminatorKind::Goto(target),
        mir::TerminatorKind::SwitchBool {
            condition,
            then_target,
            else_target,
        } => out::TerminatorKind::SwitchBool {
            condition: lower_operand(condition, locals)?,
            then_target,
            else_target,
        },
        mir::TerminatorKind::Call {
            callee,
            args,
            destinations,
            target: next,
        } => match callee {
            hir::Callee::Function(id) => out::TerminatorKind::Call {
                target: out::CallTarget::Function(id),
                args: args
                    .into_iter()
                    .map(|argument| lower_operand(argument, locals))
                    .collect::<Result<Vec<_>, _>>()?,
                destinations: destinations.into_iter().map(lower_place).collect(),
                next,
            },
            hir::Callee::Closure(id) => {
                return Err(Diagnostic::backend(format!(
                    "local function {} survived MIR inlining",
                    id.0
                )));
            }
            hir::Callee::Builtin(builtin @ (hir::Builtin::Print | hir::Builtin::Println)) => {
                return lower_print_call(
                    builtin,
                    args,
                    destinations,
                    next,
                    provenance,
                    locals,
                    original_block_count,
                    extra_blocks,
                    panic,
                );
            }
            hir::Callee::Builtin(hir::Builtin::Panic) => {
                return lower_panic_call(args, destinations, next, provenance, locals, panic);
            }
            hir::Callee::Builtin(
                builtin @ (hir::Builtin::SliceI64Index
                | hir::Builtin::SliceI64Range
                | hir::Builtin::SliceI64Set
                | hir::Builtin::SliceI64Make
                | hir::Builtin::SliceI64Nil
                | hir::Builtin::SliceI64IsNil
                | hir::Builtin::SliceI64Len
                | hir::Builtin::SliceI64Cap
                | hir::Builtin::SliceI64Append
                | hir::Builtin::SliceU8Make
                | hir::Builtin::SliceU8Set
                | hir::Builtin::SliceU8AppendSlice
                | hir::Builtin::SliceU8AppendString
                | hir::Builtin::SliceU8Copy
                | hir::Builtin::SliceU8CopyString
                | hir::Builtin::SliceU8Len
                | hir::Builtin::SliceU8Index
                | hir::Builtin::SliceU8Range
                | hir::Builtin::SliceU8Nil
                | hir::Builtin::SliceU8IsNil
                | hir::Builtin::SliceI64Copy
                | hir::Builtin::SliceI64Clear
                | hir::Builtin::SliceBoolIndex
                | hir::Builtin::SliceBoolSet
                | hir::Builtin::SliceBoolNil
                | hir::Builtin::SliceBoolIsNil
                | hir::Builtin::SliceGoStringIndex
                | hir::Builtin::SliceGoStringRange
                | hir::Builtin::SliceGoStringSet
                | hir::Builtin::SliceGoStringMake
                | hir::Builtin::SliceGoStringNil
                | hir::Builtin::SliceGoStringIsNil
                | hir::Builtin::SliceGoStringLen
                | hir::Builtin::SliceGoStringCap
                | hir::Builtin::SliceGoStringAppend
                | hir::Builtin::SliceGoStringCopy
                | hir::Builtin::SliceGoStringClear
                | hir::Builtin::AggregateSliceMake
                | hir::Builtin::AggregateSliceNil
                | hir::Builtin::AggregateSliceIsNil
                | hir::Builtin::AggregateSliceLen
                | hir::Builtin::AggregateSliceIndexTagged
                | hir::Builtin::AggregateSliceSetTagged
                | hir::Builtin::SnapshotFunctionSliceAppend
                | hir::Builtin::SnapshotFunctionSliceCall
                | hir::Builtin::StringFromRune
                | hir::Builtin::StringFromSliceU8
                | hir::Builtin::StringFromSliceRunes
                | hir::Builtin::StringToSliceRunes
                | hir::Builtin::StringLen
                | hir::Builtin::StringIndex
                | hir::Builtin::StringRange
                | hir::Builtin::StringRangeCount
                | hir::Builtin::StringRangeIndexAt
                | hir::Builtin::StringRangeRuneAt
                | hir::Builtin::MapStringI64Nil
                | hir::Builtin::MapStringI64Make
                | hir::Builtin::MapStringI64Len
                | hir::Builtin::MapStringI64Get
                | hir::Builtin::MapStringI64Lookup
                | hir::Builtin::MapStringI64Contains
                | hir::Builtin::MapStringI64Set
                | hir::Builtin::MapStringI64Delete
                | hir::Builtin::MapStringI64Clear
                | hir::Builtin::MapStringI64IsNil
                | hir::Builtin::MapStringI64KeyAt
                | hir::Builtin::MapStringI64RangeKeys
                | hir::Builtin::MapI64GoStringNil
                | hir::Builtin::MapI64GoStringMake
                | hir::Builtin::MapI64GoStringLen
                | hir::Builtin::MapI64GoStringGet
                | hir::Builtin::MapI64GoStringLookup
                | hir::Builtin::MapI64GoStringContains
                | hir::Builtin::MapI64GoStringSet
                | hir::Builtin::MapI64GoStringDelete
                | hir::Builtin::MapI64GoStringClear
                | hir::Builtin::MapI64GoStringIsNil
                | hir::Builtin::MapI64GoStringRangeKeys
                | hir::Builtin::AggregateMapMake
                | hir::Builtin::AggregateMapLen
                | hir::Builtin::AggregateMapGetTagged
                | hir::Builtin::AggregateMapContains
                | hir::Builtin::AggregateMapSetTagged
                | hir::Builtin::PointerI64Nil
                | hir::Builtin::PointerI64New
                | hir::Builtin::PointerI64Get
                | hir::Builtin::PointerI64Set
                | hir::Builtin::PointerI64IsNil
                | hir::Builtin::PointerI64Equal
                | hir::Builtin::PointerStructI64Nil
                | hir::Builtin::PointerStructI64New
                | hir::Builtin::PointerStructI64Get
                | hir::Builtin::PointerStructI64Set
                | hir::Builtin::PointerStructI64IsNil
                | hir::Builtin::PointerStructI64Equal
                | hir::Builtin::AggregatePointerNil
                | hir::Builtin::AggregatePointerNew
                | hir::Builtin::AggregatePointerSnapshot
                | hir::Builtin::AggregatePointerIsNil
                | hir::Builtin::InterfaceNil
                | hir::Builtin::InterfaceBoxBool
                | hir::Builtin::InterfaceBoxF64
                | hir::Builtin::InterfaceBoxI64
                | hir::Builtin::InterfaceBoxGoString
                | hir::Builtin::InterfaceBoxGoSliceGoString
                | hir::Builtin::InterfaceBoxStructI64
                | hir::Builtin::InterfaceBoxPointerI64
                | hir::Builtin::InterfaceBoxPointerStructI64
                | hir::Builtin::InterfaceBoxAggregate
                | hir::Builtin::InterfaceBoxComparableAggregate
                | hir::Builtin::InterfaceEqual
                | hir::Builtin::InterfaceIsNil
                | hir::Builtin::InterfaceIsType
                | hir::Builtin::InterfaceIsRuntimeError
                | hir::Builtin::InterfaceAssert
                | hir::Builtin::InterfaceSatisfies
                | hir::Builtin::InterfaceSatisfiesRuntimeError
                | hir::Builtin::InterfaceSatisfiesNonNil
                | hir::Builtin::InterfaceUnboxBool
                | hir::Builtin::InterfaceUnboxF64
                | hir::Builtin::InterfaceUnboxI64
                | hir::Builtin::InterfaceUnboxGoString
                | hir::Builtin::InterfaceUnboxGoSliceGoString
                | hir::Builtin::InterfaceStructI64Get
                | hir::Builtin::InterfaceUnboxPointerI64
                | hir::Builtin::InterfaceUnboxPointerStructI64
                | hir::Builtin::InterfaceUnboxAggregate
                | hir::Builtin::FunctionNil
                | hir::Builtin::FunctionIsNil
                | hir::Builtin::ChannelI64Nil
                | hir::Builtin::ChannelI64Make
                | hir::Builtin::ChannelI64Len
                | hir::Builtin::ChannelI64Cap
                | hir::Builtin::ChannelI64Send
                | hir::Builtin::ChannelI64ReceiveValue
                | hir::Builtin::ChannelI64Receive
                | hir::Builtin::ChannelI64Close
                | hir::Builtin::ChannelI64IsNil
                | hir::Builtin::ChannelI64TrySend
                | hir::Builtin::ChannelI64TryReceive
                | hir::Builtin::ChannelGoStringNil
                | hir::Builtin::ChannelGoStringMake
                | hir::Builtin::ChannelGoStringLen
                | hir::Builtin::ChannelGoStringCap
                | hir::Builtin::ChannelGoStringSend
                | hir::Builtin::ChannelGoStringReceiveValue
                | hir::Builtin::ChannelGoStringReceive
                | hir::Builtin::ChannelGoStringClose
                | hir::Builtin::ChannelGoStringIsNil
                | hir::Builtin::ChannelGoStringTrySend
                | hir::Builtin::ChannelGoStringTryReceive
                | hir::Builtin::ChannelGoChannelI64Nil
                | hir::Builtin::ChannelGoChannelI64Make
                | hir::Builtin::ChannelGoChannelI64Len
                | hir::Builtin::ChannelGoChannelI64Cap
                | hir::Builtin::ChannelGoChannelI64Send
                | hir::Builtin::ChannelGoChannelI64ReceiveValue
                | hir::Builtin::ChannelGoChannelI64Receive
                | hir::Builtin::ChannelGoChannelI64Close
                | hir::Builtin::ChannelGoChannelI64IsNil
                | hir::Builtin::ChannelGoChannelI64TrySend
                | hir::Builtin::ChannelGoChannelI64TryReceive),
            ) => out::TerminatorKind::Call {
                target: out::CallTarget::Runtime(match builtin {
                    hir::Builtin::SliceI64Index => RuntimeOp::GoSliceI64Index,
                    hir::Builtin::SliceI64Range => RuntimeOp::GoSliceI64Range,
                    hir::Builtin::SliceI64Set => RuntimeOp::GoSliceI64Set,
                    hir::Builtin::SliceI64Make => RuntimeOp::GoSliceI64Make,
                    hir::Builtin::SliceI64Nil => RuntimeOp::GoSliceI64Nil,
                    hir::Builtin::SliceI64IsNil => RuntimeOp::GoSliceI64IsNil,
                    hir::Builtin::SliceI64Len => RuntimeOp::GoSliceI64Len,
                    hir::Builtin::SliceI64Cap => RuntimeOp::GoSliceI64Cap,
                    hir::Builtin::SliceI64Append => RuntimeOp::GoSliceI64Append,
                    hir::Builtin::SliceU8Make => RuntimeOp::GoSliceU8Make,
                    hir::Builtin::SliceU8Set => RuntimeOp::GoSliceU8Set,
                    hir::Builtin::SliceU8AppendSlice => RuntimeOp::GoSliceU8AppendSlice,
                    hir::Builtin::SliceU8AppendString => RuntimeOp::GoSliceU8AppendString,
                    hir::Builtin::SliceU8Copy => RuntimeOp::GoSliceU8Copy,
                    hir::Builtin::SliceU8CopyString => RuntimeOp::GoSliceU8CopyString,
                    hir::Builtin::SliceU8Len => RuntimeOp::GoSliceU8Len,
                    hir::Builtin::SliceU8Index => RuntimeOp::GoSliceU8Index,
                    hir::Builtin::SliceU8Range => RuntimeOp::GoSliceU8Range,
                    hir::Builtin::SliceU8Nil => RuntimeOp::GoSliceU8Nil,
                    hir::Builtin::SliceU8IsNil => RuntimeOp::GoSliceU8IsNil,
                    hir::Builtin::SliceI64Copy => RuntimeOp::GoSliceI64Copy,
                    hir::Builtin::SliceI64Clear => RuntimeOp::GoSliceI64Clear,
                    hir::Builtin::SliceBoolIndex => RuntimeOp::GoSliceBoolIndex,
                    hir::Builtin::SliceBoolSet => RuntimeOp::GoSliceBoolSet,
                    hir::Builtin::SliceBoolNil => RuntimeOp::GoSliceBoolNil,
                    hir::Builtin::SliceBoolIsNil => RuntimeOp::GoSliceBoolIsNil,
                    hir::Builtin::SliceGoStringIndex => RuntimeOp::GoSliceGoStringIndex,
                    hir::Builtin::SliceGoStringRange => RuntimeOp::GoSliceGoStringRange,
                    hir::Builtin::SliceGoStringSet => RuntimeOp::GoSliceGoStringSet,
                    hir::Builtin::SliceGoStringMake => RuntimeOp::GoSliceGoStringMake,
                    hir::Builtin::SliceGoStringNil => RuntimeOp::GoSliceGoStringNil,
                    hir::Builtin::SliceGoStringIsNil => RuntimeOp::GoSliceGoStringIsNil,
                    hir::Builtin::SliceGoStringLen => RuntimeOp::GoSliceGoStringLen,
                    hir::Builtin::SliceGoStringCap => RuntimeOp::GoSliceGoStringCap,
                    hir::Builtin::SliceGoStringAppend => RuntimeOp::GoSliceGoStringAppend,
                    hir::Builtin::SliceGoStringCopy => RuntimeOp::GoSliceGoStringCopy,
                    hir::Builtin::SliceGoStringClear => RuntimeOp::GoSliceGoStringClear,
                    hir::Builtin::AggregateSliceMake => RuntimeOp::GoSliceInterfaceMake,
                    hir::Builtin::AggregateSliceNil => RuntimeOp::GoSliceInterfaceNil,
                    hir::Builtin::AggregateSliceIsNil => RuntimeOp::GoSliceInterfaceIsNil,
                    hir::Builtin::AggregateSliceLen => RuntimeOp::GoSliceInterfaceLen,
                    hir::Builtin::AggregateSliceIndexTagged => RuntimeOp::GoSliceInterfaceIndex,
                    hir::Builtin::AggregateSliceSetTagged => RuntimeOp::GoSliceInterfaceSet,
                    hir::Builtin::SnapshotFunctionSliceAppend => RuntimeOp::GoSliceI64Append,
                    hir::Builtin::SnapshotFunctionSliceCall => RuntimeOp::GoSliceI64Index,
                    hir::Builtin::StringFromRune => RuntimeOp::GoStringFromRune,
                    hir::Builtin::StringFromSliceU8 => RuntimeOp::GoStringFromSliceU8,
                    hir::Builtin::StringFromSliceRunes => RuntimeOp::GoStringFromSliceRunes,
                    hir::Builtin::StringToSliceRunes => RuntimeOp::GoStringToSliceRunes,
                    hir::Builtin::StringLen => RuntimeOp::GoStringLen,
                    hir::Builtin::StringIndex => RuntimeOp::GoStringIndex,
                    hir::Builtin::StringRange => RuntimeOp::GoStringRange,
                    hir::Builtin::StringRangeCount => RuntimeOp::GoStringRangeCount,
                    hir::Builtin::StringRangeIndexAt => RuntimeOp::GoStringRangeIndexAt,
                    hir::Builtin::StringRangeRuneAt => RuntimeOp::GoStringRangeRuneAt,
                    hir::Builtin::MapStringI64Nil => RuntimeOp::GoMapStringI64Nil,
                    hir::Builtin::MapStringI64Make => RuntimeOp::GoMapStringI64Make,
                    hir::Builtin::MapStringI64Len => RuntimeOp::GoMapStringI64Len,
                    hir::Builtin::MapStringI64Get => RuntimeOp::GoMapStringI64Get,
                    hir::Builtin::MapStringI64Contains => RuntimeOp::GoMapStringI64Contains,
                    hir::Builtin::MapStringI64Set => RuntimeOp::GoMapStringI64Set,
                    hir::Builtin::MapStringI64Delete => RuntimeOp::GoMapStringI64Delete,
                    hir::Builtin::MapStringI64Clear => RuntimeOp::GoMapStringI64Clear,
                    hir::Builtin::MapStringI64IsNil => RuntimeOp::GoMapStringI64IsNil,
                    hir::Builtin::MapStringI64KeyAt => RuntimeOp::GoMapStringI64KeyAt,
                    hir::Builtin::MapStringI64RangeKeys => RuntimeOp::GoMapStringI64RangeKeys,
                    hir::Builtin::MapI64GoStringNil => RuntimeOp::GoMapI64GoStringNil,
                    hir::Builtin::MapI64GoStringMake => RuntimeOp::GoMapI64GoStringMake,
                    hir::Builtin::MapI64GoStringLen => RuntimeOp::GoMapI64GoStringLen,
                    hir::Builtin::MapI64GoStringGet => RuntimeOp::GoMapI64GoStringGet,
                    hir::Builtin::MapI64GoStringContains => RuntimeOp::GoMapI64GoStringContains,
                    hir::Builtin::MapI64GoStringSet => RuntimeOp::GoMapI64GoStringSet,
                    hir::Builtin::MapI64GoStringDelete => RuntimeOp::GoMapI64GoStringDelete,
                    hir::Builtin::MapI64GoStringClear => RuntimeOp::GoMapI64GoStringClear,
                    hir::Builtin::MapI64GoStringIsNil => RuntimeOp::GoMapI64GoStringIsNil,
                    hir::Builtin::MapI64GoStringRangeKeys => RuntimeOp::GoMapI64GoStringRangeKeys,
                    hir::Builtin::AggregateMapMake => RuntimeOp::GoMapStringInterfaceMake,
                    hir::Builtin::AggregateMapLen => RuntimeOp::GoMapStringInterfaceLen,
                    hir::Builtin::AggregateMapGetTagged => RuntimeOp::GoMapStringInterfaceGet,
                    hir::Builtin::AggregateMapContains => RuntimeOp::GoMapStringInterfaceContains,
                    hir::Builtin::AggregateMapSetTagged => RuntimeOp::GoMapStringInterfaceSet,
                    hir::Builtin::PointerI64Nil => RuntimeOp::GoPointerI64Nil,
                    hir::Builtin::PointerI64New => RuntimeOp::GoPointerI64New,
                    hir::Builtin::PointerI64Get => RuntimeOp::GoPointerI64Get,
                    hir::Builtin::PointerI64Set => RuntimeOp::GoPointerI64Set,
                    hir::Builtin::PointerI64IsNil => RuntimeOp::GoPointerI64IsNil,
                    hir::Builtin::PointerI64Equal => RuntimeOp::GoPointerI64Equal,
                    hir::Builtin::PointerStructI64Nil => RuntimeOp::GoPointerStructI64Nil,
                    hir::Builtin::PointerStructI64New => RuntimeOp::GoPointerStructI64New,
                    hir::Builtin::PointerStructI64Get => RuntimeOp::GoPointerStructI64Get,
                    hir::Builtin::PointerStructI64Set => RuntimeOp::GoPointerStructI64Set,
                    hir::Builtin::PointerStructI64IsNil => RuntimeOp::GoPointerStructI64IsNil,
                    hir::Builtin::PointerStructI64Equal => RuntimeOp::GoPointerStructI64Equal,
                    hir::Builtin::AggregatePointerNil => RuntimeOp::GoInterfaceNil,
                    hir::Builtin::AggregatePointerNew => RuntimeOp::GoInterfaceBoxAggregate,
                    hir::Builtin::AggregatePointerSnapshot => RuntimeOp::GoInterfaceUnboxAggregate,
                    hir::Builtin::AggregatePointerIsNil => RuntimeOp::GoInterfaceIsNil,
                    hir::Builtin::InterfaceNil => RuntimeOp::GoInterfaceNil,
                    hir::Builtin::InterfaceBoxBool => RuntimeOp::GoInterfaceBoxBool,
                    hir::Builtin::InterfaceBoxF64 => RuntimeOp::GoInterfaceBoxF64,
                    hir::Builtin::InterfaceBoxI64 => RuntimeOp::GoInterfaceBoxI64,
                    hir::Builtin::InterfaceBoxGoString => RuntimeOp::GoInterfaceBoxGoString,
                    hir::Builtin::InterfaceBoxGoSliceGoString => {
                        RuntimeOp::GoInterfaceBoxGoSliceGoString
                    }
                    hir::Builtin::InterfaceBoxStructI64 => RuntimeOp::GoInterfaceBoxStructI64,
                    hir::Builtin::InterfaceBoxPointerI64 => RuntimeOp::GoInterfaceBoxPointerI64,
                    hir::Builtin::InterfaceBoxPointerStructI64 => {
                        RuntimeOp::GoInterfaceBoxPointerStructI64
                    }
                    hir::Builtin::InterfaceBoxAggregate => RuntimeOp::GoInterfaceBoxAggregate,
                    hir::Builtin::InterfaceBoxComparableAggregate => {
                        RuntimeOp::GoInterfaceBoxComparableAggregate
                    }
                    hir::Builtin::InterfaceEqual => RuntimeOp::GoInterfaceEqual,
                    hir::Builtin::InterfaceIsNil => RuntimeOp::GoInterfaceIsNil,
                    hir::Builtin::InterfaceIsType => RuntimeOp::GoInterfaceIsType,
                    hir::Builtin::InterfaceIsRuntimeError => RuntimeOp::GoInterfaceIsRuntimeError,
                    hir::Builtin::InterfaceUnboxBool => RuntimeOp::GoInterfaceUnboxBool,
                    hir::Builtin::InterfaceUnboxF64 => RuntimeOp::GoInterfaceUnboxF64,
                    hir::Builtin::InterfaceUnboxI64 => RuntimeOp::GoInterfaceUnboxI64,
                    hir::Builtin::InterfaceUnboxGoString => RuntimeOp::GoInterfaceUnboxGoString,
                    hir::Builtin::InterfaceUnboxGoSliceGoString => {
                        RuntimeOp::GoInterfaceUnboxGoSliceGoString
                    }
                    hir::Builtin::InterfaceStructI64Get => RuntimeOp::GoInterfaceStructI64Get,
                    hir::Builtin::InterfaceUnboxPointerI64 => RuntimeOp::GoInterfaceUnboxPointerI64,
                    hir::Builtin::InterfaceUnboxPointerStructI64 => {
                        RuntimeOp::GoInterfaceUnboxPointerStructI64
                    }
                    hir::Builtin::InterfaceUnboxAggregate => RuntimeOp::GoInterfaceUnboxAggregate,
                    hir::Builtin::FunctionNil => RuntimeOp::GoInterfaceNil,
                    hir::Builtin::FunctionIsNil => RuntimeOp::GoInterfaceIsNil,
                    hir::Builtin::ChannelI64Nil => RuntimeOp::GoChannelI64Nil,
                    hir::Builtin::ChannelI64Make => RuntimeOp::GoChannelI64Make,
                    hir::Builtin::ChannelI64Len => RuntimeOp::GoChannelI64Len,
                    hir::Builtin::ChannelI64Cap => RuntimeOp::GoChannelI64Cap,
                    hir::Builtin::ChannelI64Send => RuntimeOp::GoChannelI64Send,
                    hir::Builtin::ChannelI64ReceiveValue => RuntimeOp::GoChannelI64ReceiveValue,
                    hir::Builtin::ChannelI64Receive => RuntimeOp::GoChannelI64Receive,
                    hir::Builtin::ChannelI64Close => RuntimeOp::GoChannelI64Close,
                    hir::Builtin::ChannelI64IsNil => RuntimeOp::GoChannelI64IsNil,
                    hir::Builtin::ChannelI64TrySend => RuntimeOp::GoChannelI64TrySend,
                    hir::Builtin::ChannelI64TryReceive => RuntimeOp::GoChannelI64TryReceive,
                    hir::Builtin::ChannelGoStringNil => RuntimeOp::GoChannelGoStringNil,
                    hir::Builtin::ChannelGoStringMake => RuntimeOp::GoChannelGoStringMake,
                    hir::Builtin::ChannelGoStringLen => RuntimeOp::GoChannelGoStringLen,
                    hir::Builtin::ChannelGoStringCap => RuntimeOp::GoChannelGoStringCap,
                    hir::Builtin::ChannelGoStringSend => RuntimeOp::GoChannelGoStringSend,
                    hir::Builtin::ChannelGoStringReceiveValue => {
                        RuntimeOp::GoChannelGoStringReceiveValue
                    }
                    hir::Builtin::ChannelGoStringReceive => RuntimeOp::GoChannelGoStringReceive,
                    hir::Builtin::ChannelGoStringClose => RuntimeOp::GoChannelGoStringClose,
                    hir::Builtin::ChannelGoStringIsNil => RuntimeOp::GoChannelGoStringIsNil,
                    hir::Builtin::ChannelGoStringTrySend => RuntimeOp::GoChannelGoStringTrySend,
                    hir::Builtin::ChannelGoStringTryReceive => {
                        RuntimeOp::GoChannelGoStringTryReceive
                    }
                    hir::Builtin::ChannelGoChannelI64Nil => RuntimeOp::GoChannelGoChannelI64Nil,
                    hir::Builtin::ChannelGoChannelI64Make => RuntimeOp::GoChannelGoChannelI64Make,
                    hir::Builtin::ChannelGoChannelI64Len => RuntimeOp::GoChannelGoChannelI64Len,
                    hir::Builtin::ChannelGoChannelI64Cap => RuntimeOp::GoChannelGoChannelI64Cap,
                    hir::Builtin::ChannelGoChannelI64Send => RuntimeOp::GoChannelGoChannelI64Send,
                    hir::Builtin::ChannelGoChannelI64ReceiveValue => {
                        RuntimeOp::GoChannelGoChannelI64ReceiveValue
                    }
                    hir::Builtin::ChannelGoChannelI64Receive => {
                        RuntimeOp::GoChannelGoChannelI64Receive
                    }
                    hir::Builtin::ChannelGoChannelI64Close => RuntimeOp::GoChannelGoChannelI64Close,
                    hir::Builtin::ChannelGoChannelI64IsNil => RuntimeOp::GoChannelGoChannelI64IsNil,
                    hir::Builtin::ChannelGoChannelI64TrySend => {
                        RuntimeOp::GoChannelGoChannelI64TrySend
                    }
                    hir::Builtin::ChannelGoChannelI64TryReceive => {
                        RuntimeOp::GoChannelGoChannelI64TryReceive
                    }
                    hir::Builtin::MapStringI64Lookup | hir::Builtin::MapI64GoStringLookup => {
                        return Err(Diagnostic::backend(
                            "map comma-ok lookup survived MIR expansion",
                        ));
                    }
                    hir::Builtin::InterfaceAssert => {
                        return Err(Diagnostic::backend(
                            "interface assertion survived MIR expansion",
                        ));
                    }
                    hir::Builtin::InterfaceSatisfies => {
                        return Err(Diagnostic::backend(
                            "interface satisfaction assertion survived MIR expansion",
                        ));
                    }
                    hir::Builtin::InterfaceSatisfiesRuntimeError => {
                        return Err(Diagnostic::backend(
                            "runtime-error interface satisfaction survived MIR expansion",
                        ));
                    }
                    hir::Builtin::InterfaceSatisfiesNonNil => {
                        return Err(Diagnostic::backend(
                            "implied interface satisfaction survived MIR expansion",
                        ));
                    }
                    hir::Builtin::Print | hir::Builtin::Println | hir::Builtin::Panic => {
                        return Err(Diagnostic::backend(
                            "non-slice builtin reached slice representation lowering",
                        ));
                    }
                }),
                args: args
                    .into_iter()
                    .map(|argument| lower_operand(argument, locals))
                    .collect::<Result<Vec<_>, _>>()?,
                destinations: destinations.into_iter().map(lower_place).collect(),
                next,
            },
        },
        mir::TerminatorKind::SpawnEmpty { target } => out::TerminatorKind::Goto(target),
        mir::TerminatorKind::Return(values) => out::TerminatorKind::Return(
            values
                .into_iter()
                .map(|value| lower_operand(value, locals))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        mir::TerminatorKind::Unreachable => out::TerminatorKind::Unreachable,
    };
    Ok(finish_terminator(kind, provenance, panic))
}

fn lower_panic_call(
    args: Vec<mir::Operand>,
    destinations: Vec<mir::Place>,
    next: out::BasicBlockId,
    provenance: out::Provenance,
    locals: &[out::LocalDecl],
    panic: mir::PanicEdge,
) -> Result<out::Terminator, Diagnostic> {
    if !destinations.is_empty() {
        return Err(Diagnostic::backend(
            "Go panic builtin unexpectedly has a result destination",
        ));
    }
    let [argument]: [mir::Operand; 1] = args.try_into().map_err(|args: Vec<_>| {
        Diagnostic::backend(format!(
            "Go panic builtin reached Rust lowering with {} arguments",
            args.len()
        ))
    })?;
    let argument_ty = mir_operand_type(&argument, locals)?;
    if argument_ty != out::RustType::GoInterface {
        return Err(Diagnostic::backend(format!(
            "verified Go panic payload is not an empty interface: {argument_ty:?}"
        )));
    }
    Ok(finish_terminator(
        out::TerminatorKind::Call {
            target: out::CallTarget::Runtime(RuntimeOp::PanicGoInterface),
            args: vec![lower_operand(argument, locals)?],
            destinations: Vec::new(),
            next,
        },
        provenance,
        panic,
    ))
}

pub(super) fn lower_operand(
    operand: mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::Operand, Diagnostic> {
    match operand {
        mir::Operand::Read(place) => {
            let place = lower_place(place);
            let local = local(locals, place.local)?;
            let op = local.ty.conservative_read_op().ok_or_else(|| {
                Diagnostic::backend(format!(
                    "Rust lowering cannot read unit local {}",
                    place.local.0
                ))
            })?;
            Ok(out::Operand::Read { place, op })
        }
        mir::Operand::Constant(value, ty) => {
            Ok(out::Operand::Constant(lower_constant(value, &ty)?))
        }
        mir::Operand::Unit => Ok(out::Operand::Unit),
    }
}

pub(super) fn mir_operand_type(
    operand: &mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::RustType, Diagnostic> {
    match operand {
        mir::Operand::Read(place) => Ok(local(locals, place.local)?.ty.clone()),
        mir::Operand::Constant(_, ty) => lower_type(ty),
        mir::Operand::Unit => Ok(out::RustType::Unit),
    }
}

fn local(locals: &[out::LocalDecl], id: out::LocalId) -> Result<&out::LocalDecl, Diagnostic> {
    locals
        .get(id.0 as usize)
        .ok_or_else(|| Diagnostic::backend(format!("invalid local during Rust lowering: {}", id.0)))
}

fn lower_place(place: mir::Place) -> out::Place {
    out::Place { local: place.local }
}
