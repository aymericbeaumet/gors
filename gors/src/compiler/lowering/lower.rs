//! Mandatory conversion from normalized Go MIR to explicit Rust IR.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use crate::compiler::types::{
    ComplexTy, ConstValue, FloatTy, IntTy, Signature as GoSignature, Ty, parse_go_float,
};
use gors_runtime_abi::{PrimitiveOp, RuntimeOp};

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
    let panic_cleanup = function.panic_cleanup.map(|cleanup| out::PanicCleanup {
        entry: cleanup.entry,
        active: cleanup.active,
    });
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
        mir::RvalueKind::SliceLiteralI64(elements) => {
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
        mir::RvalueKind::ArrayLiteralI64(elements) => out::RvalueKind::Use(out::Operand::Constant(
            out::Constant::StaticI64Array(elements),
        )),
        mir::RvalueKind::ArrayLiteral { elements, ty } => {
            let ty = lower_type(&ty)?;
            if ty.scalar_array_parts().is_none() {
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
            let out::RustType::StructI64(length) = lower_type(&ty)? else {
                return Err(Diagnostic::backend(
                    "non-integer struct reached integer struct representation lowering",
                ));
            };
            if fields.len()
                != usize::try_from(length).map_err(|_| {
                    Diagnostic::backend("struct representation length does not fit usize")
                })?
            {
                return Err(Diagnostic::backend(
                    "struct field count changed during representation lowering",
                ));
            }
            out::RvalueKind::StructLiteralI64(
                fields
                    .into_iter()
                    .map(|field| lower_operand(field, locals))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        mir::RvalueKind::StructField { structure, field } => {
            let out::RustType::StructI64(length) = mir_operand_type(&structure, locals)? else {
                return Err(Diagnostic::backend(
                    "non-integer struct reached integer field representation lowering",
                ));
            };
            if u64::from(field) >= length {
                return Err(Diagnostic::backend(
                    "struct field index is outside its representation",
                ));
            }
            out::RvalueKind::StructFieldI64 {
                structure: lower_operand(structure, locals)?,
                field,
            }
        }
        mir::RvalueKind::StructSet {
            structure,
            field,
            value,
        } => {
            let out::RustType::StructI64(length) = mir_operand_type(&structure, locals)? else {
                return Err(Diagnostic::backend(
                    "non-integer struct reached integer field update lowering",
                ));
            };
            if u64::from(field) >= length || mir_operand_type(&value, locals)? != out::RustType::I64
            {
                return Err(Diagnostic::backend(
                    "invalid integer struct field update reached representation lowering",
                ));
            }
            out::RvalueKind::StructSetI64 {
                structure: lower_operand(structure, locals)?,
                field,
                value: lower_operand(value, locals)?,
            }
        }
        mir::RvalueKind::Unary { op, operand, ty } => {
            let operand_ty = mir_operand_type(&operand, locals)?;
            let operand = lower_operand(operand, locals)?;
            match lower_unary_op(op, operand_ty, lower_type(&ty)?)? {
                Some(op) => out::RvalueKind::Unary { op, operand },
                None => out::RvalueKind::Use(operand),
            }
        }
        mir::RvalueKind::Conversion { operand, from, ty } => {
            let from = lower_type(&from)?;
            let to = lower_type(&ty)?;
            if from != to {
                return Err(Diagnostic::backend(format!(
                    "representation-preserving conversion changed Rust type from {from:?} to {to:?}"
                )));
            }
            out::RvalueKind::Use(lower_operand(operand, locals)?)
        }
        mir::RvalueKind::RecoverCompareNil { state, equal } => out::RvalueKind::RecoverCompareNil {
            state: lower_place(state),
            equal,
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
                    out::RustType::ArrayI64(_) | out::RustType::StructI64(_)
                )
                && result_ty == out::RustType::Bool
            {
                out::RvalueKind::AggregateEqualI64 {
                    left: lower_operand(left, locals)?,
                    right: lower_operand(right, locals)?,
                    equal: op == hir::BinaryOp::Equal,
                }
            } else {
                out::RvalueKind::Binary {
                    op: lower_binary_op(op, left_ty, right_ty, result_ty)?,
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
                | hir::Builtin::SliceI64Len
                | hir::Builtin::SliceI64Cap
                | hir::Builtin::SliceI64Append
                | hir::Builtin::SliceU8AppendSlice
                | hir::Builtin::SliceU8AppendString
                | hir::Builtin::SliceU8CopyString
                | hir::Builtin::SliceI64Copy
                | hir::Builtin::SliceI64Clear
                | hir::Builtin::StringFromSliceU8
                | hir::Builtin::StringLen
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
                | hir::Builtin::PointerI64Nil
                | hir::Builtin::PointerI64New
                | hir::Builtin::PointerI64Get
                | hir::Builtin::PointerI64Set
                | hir::Builtin::PointerI64IsNil
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
                | hir::Builtin::ChannelI64TryReceive),
            ) => out::TerminatorKind::Call {
                target: out::CallTarget::Runtime(match builtin {
                    hir::Builtin::SliceI64Index => RuntimeOp::GoSliceI64Index,
                    hir::Builtin::SliceI64Range => RuntimeOp::GoSliceI64Range,
                    hir::Builtin::SliceI64Set => RuntimeOp::GoSliceI64Set,
                    hir::Builtin::SliceI64Make => RuntimeOp::GoSliceI64Make,
                    hir::Builtin::SliceI64Len => RuntimeOp::GoSliceI64Len,
                    hir::Builtin::SliceI64Cap => RuntimeOp::GoSliceI64Cap,
                    hir::Builtin::SliceI64Append => RuntimeOp::GoSliceI64Append,
                    hir::Builtin::SliceU8AppendSlice => RuntimeOp::GoSliceU8AppendSlice,
                    hir::Builtin::SliceU8AppendString => RuntimeOp::GoSliceU8AppendString,
                    hir::Builtin::SliceU8CopyString => RuntimeOp::GoSliceU8CopyString,
                    hir::Builtin::SliceI64Copy => RuntimeOp::GoSliceI64Copy,
                    hir::Builtin::SliceI64Clear => RuntimeOp::GoSliceI64Clear,
                    hir::Builtin::StringFromSliceU8 => RuntimeOp::GoStringFromSliceU8,
                    hir::Builtin::StringLen => RuntimeOp::GoStringLen,
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
                    hir::Builtin::PointerI64Nil => RuntimeOp::GoPointerI64Nil,
                    hir::Builtin::PointerI64New => RuntimeOp::GoPointerI64New,
                    hir::Builtin::PointerI64Get => RuntimeOp::GoPointerI64Get,
                    hir::Builtin::PointerI64Set => RuntimeOp::GoPointerI64Set,
                    hir::Builtin::PointerI64IsNil => RuntimeOp::GoPointerI64IsNil,
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
                    hir::Builtin::MapStringI64Lookup => {
                        return Err(Diagnostic::backend(
                            "map comma-ok lookup survived MIR expansion",
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
    let operation = match mir_operand_type(&argument, locals)? {
        out::RustType::Bool => RuntimeOp::PanicBool,
        out::RustType::I64 => RuntimeOp::PanicI64,
        out::RustType::GoString => RuntimeOp::PanicGoString,
        out::RustType::F64
        | out::RustType::Complex128
        | out::RustType::ArrayI64(_)
        | out::RustType::ArrayBool(_)
        | out::RustType::ArrayF64(_)
        | out::RustType::ArrayGoString(_)
        | out::RustType::StructI64(_)
        | out::RustType::GoSliceI64
        | out::RustType::GoSliceU8
        | out::RustType::GoMapStringI64
        | out::RustType::GoPointerI64
        | out::RustType::GoPointerStructI64
        | out::RustType::GoChannelI64 => {
            return Err(Diagnostic::backend(
                "unsupported numeric panic payload reached Rust lowering",
            ));
        }
        out::RustType::Unit => {
            return Err(Diagnostic::backend(
                "unit value reached Rust panic representation lowering",
            ));
        }
    };
    Ok(finish_terminator(
        out::TerminatorKind::Call {
            target: out::CallTarget::Runtime(operation),
            args: vec![lower_operand(argument, locals)?],
            destinations: Vec::new(),
            next,
        },
        provenance,
        panic,
    ))
}

#[allow(clippy::too_many_arguments)]
fn lower_print_call(
    builtin: hir::Builtin,
    args: Vec<mir::Operand>,
    destinations: Vec<mir::Place>,
    next: out::BasicBlockId,
    provenance: out::Provenance,
    locals: &[out::LocalDecl],
    original_block_count: usize,
    extra_blocks: &mut Vec<out::BasicBlock>,
    panic: mir::PanicEdge,
) -> Result<out::Terminator, Diagnostic> {
    if !destinations.is_empty() {
        return Err(Diagnostic::backend(
            "Go print builtin unexpectedly has a result destination",
        ));
    }

    let argument_types = args
        .iter()
        .map(|argument| mir_operand_type(argument, locals))
        .collect::<Result<Vec<_>, _>>()?;
    let newline = builtin == hir::Builtin::Println;
    let mut calls = Vec::with_capacity(
        args.len()
            .saturating_mul(usize::from(newline).saturating_add(1))
            .saturating_add(usize::from(newline)),
    );
    let mut arguments = args.into_iter().zip(argument_types).peekable();
    while let Some((argument, ty)) = arguments.next() {
        let operation = match ty {
            out::RustType::Bool => RuntimeOp::PrintBool,
            out::RustType::I64 => RuntimeOp::PrintI64,
            out::RustType::GoString => RuntimeOp::PrintGoString,
            out::RustType::F64
            | out::RustType::Complex128
            | out::RustType::ArrayI64(_)
            | out::RustType::ArrayBool(_)
            | out::RustType::ArrayF64(_)
            | out::RustType::ArrayGoString(_)
            | out::RustType::StructI64(_)
            | out::RustType::GoSliceI64
            | out::RustType::GoSliceU8
            | out::RustType::GoMapStringI64
            | out::RustType::GoPointerI64
            | out::RustType::GoPointerStructI64
            | out::RustType::GoChannelI64 => {
                return Err(Diagnostic::backend(
                    "numeric print operation reached lowering without a runtime ABI operation",
                ));
            }
            out::RustType::Unit => {
                return Err(Diagnostic::backend(
                    "unit value reached Rust print representation lowering",
                ));
            }
        };
        calls.push((operation, vec![lower_operand(argument, locals)?]));
        if newline && arguments.peek().is_some() {
            calls.push((RuntimeOp::PrintSpace, Vec::new()));
        }
    }
    if newline {
        calls.push((RuntimeOp::PrintNewline, Vec::new()));
    }
    if calls.is_empty() {
        return Ok(finish_terminator(
            out::TerminatorKind::Goto(next),
            provenance,
            panic,
        ));
    }

    let tail_count = calls.len().saturating_sub(1);
    let mut tail_ids = Vec::with_capacity(tail_count);
    for offset in 0..tail_count {
        let index = original_block_count
            .checked_add(extra_blocks.len())
            .and_then(|index| index.checked_add(offset))
            .ok_or_else(|| Diagnostic::backend("Rust IR block count overflow"))?;
        let index = u32::try_from(index)
            .map_err(|_| Diagnostic::backend("Rust IR block count exceeds u32"))?;
        tail_ids.push(out::BasicBlockId(index));
    }

    let mut calls = calls.into_iter();
    let (first_operation, first_args) = calls
        .next()
        .ok_or_else(|| Diagnostic::backend("missing lowered Rust runtime call"))?;
    let first_next = tail_ids.first().copied().unwrap_or(next);
    for (index, ((operation, args), id)) in calls.zip(tail_ids.iter().copied()).enumerate() {
        let call_next = tail_ids
            .get(index.saturating_add(1))
            .copied()
            .unwrap_or(next);
        extra_blocks.push(out::BasicBlock {
            id,
            provenance: provenance.clone(),
            statements: Vec::new(),
            terminator: finish_terminator(
                out::TerminatorKind::Call {
                    target: out::CallTarget::Runtime(operation),
                    args,
                    destinations: Vec::new(),
                    next: call_next,
                },
                provenance.clone(),
                panic,
            ),
        });
    }

    Ok(finish_terminator(
        out::TerminatorKind::Call {
            target: out::CallTarget::Runtime(first_operation),
            args: first_args,
            destinations: Vec::new(),
            next: first_next,
        },
        provenance,
        panic,
    ))
}

fn finish_terminator(
    kind: out::TerminatorKind,
    provenance: out::Provenance,
    panic: mir::PanicEdge,
) -> out::Terminator {
    let effects = out::terminator_effects(&kind);
    out::Terminator {
        kind,
        effects,
        panic: lower_panic_edge(panic, effects),
        provenance,
    }
}

fn lower_panic_edge(edge: mir::PanicEdge, effects: out::Effects) -> out::PanicEdge {
    if !effects.may_panic {
        return out::PanicEdge::None;
    }
    match edge {
        mir::PanicEdge::Cleanup(target) => out::PanicEdge::Cleanup(target),
        mir::PanicEdge::None | mir::PanicEdge::Propagate => out::PanicEdge::Propagate,
    }
}

fn lower_operand(
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

fn lower_constant(value: ConstValue, ty: &Ty) -> Result<out::Constant, Diagnostic> {
    match (value, ty.underlying()) {
        (ConstValue::Bool(value), Ty::Bool) => Ok(out::Constant::Bool(value)),
        (ConstValue::Int(value), Ty::Int(IntTy::Int)) => value
            .parse::<i64>()
            .map(out::Constant::I64)
            .map_err(|_| Diagnostic::backend(format!("Go int is outside Rust IR i64: {value}"))),
        (ConstValue::String(bytes), Ty::String) => Ok(out::Constant::RuntimeStaticBytes {
            op: RuntimeOp::GoStringFromStatic,
            bytes,
        }),
        (ConstValue::Float(value), Ty::Float(FloatTy::Float64)) => parse_go_float(&value)
            .map(f64::to_bits)
            .map(out::Constant::F64)
            .ok_or_else(|| Diagnostic::backend(format!("invalid Go float64 constant: {value}"))),
        (ConstValue::Int(value), Ty::Float(FloatTy::Float64)) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(f64::to_bits)
            .map(out::Constant::F64)
            .ok_or_else(|| {
                Diagnostic::backend(format!("invalid Go integer-to-float constant: {value}"))
            }),
        (ConstValue::Int(value), Ty::Complex(ComplexTy::Complex128)) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|real| out::Constant::Complex128 {
                real: real.to_bits(),
                imag: 0.0_f64.to_bits(),
            })
            .ok_or_else(|| {
                Diagnostic::backend(format!("invalid Go integer-to-complex constant: {value}"))
            }),
        (ConstValue::Float(value), Ty::Complex(ComplexTy::Complex128)) => parse_go_float(&value)
            .filter(|value| value.is_finite())
            .map(|real| out::Constant::Complex128 {
                real: real.to_bits(),
                imag: 0.0_f64.to_bits(),
            })
            .ok_or_else(|| {
                Diagnostic::backend(format!("invalid Go float-to-complex constant: {value}"))
            }),
        (ConstValue::Complex { real, imag }, Ty::Complex(ComplexTy::Complex128)) => {
            let real = parse_go_float(&real)
                .ok_or_else(|| Diagnostic::backend("invalid real complex128 component"))?;
            let imag = parse_go_float(&imag)
                .ok_or_else(|| Diagnostic::backend("invalid imaginary complex128 component"))?;
            Ok(out::Constant::Complex128 {
                real: real.to_bits(),
                imag: imag.to_bits(),
            })
        }
        (value, ty) => Err(Diagnostic::backend(format!(
            "invalid constant reached Rust lowering: {value:?} as {ty:?}"
        ))),
    }
}

fn lower_unary_op(
    op: hir::UnaryOp,
    operand: out::RustType,
    result: out::RustType,
) -> Result<Option<out::ValueOp>, Diagnostic> {
    let lowered = match (op, operand, result) {
        (hir::UnaryOp::Positive, out::RustType::I64, out::RustType::I64) => None,
        (hir::UnaryOp::Positive, out::RustType::F64, out::RustType::F64)
        | (hir::UnaryOp::Positive, out::RustType::Complex128, out::RustType::Complex128) => None,
        (hir::UnaryOp::Negative, out::RustType::I64, out::RustType::I64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::IntWrappingNeg))
        }
        (hir::UnaryOp::Negative, out::RustType::F64, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::FloatNeg))
        }
        (hir::UnaryOp::Negative, out::RustType::Complex128, out::RustType::Complex128) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexNeg))
        }
        (hir::UnaryOp::Not, out::RustType::Bool, out::RustType::Bool) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::BoolNot))
        }
        (hir::UnaryOp::BitNot, out::RustType::I64, out::RustType::I64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::IntBitNot))
        }
        (hir::UnaryOp::Real, out::RustType::Complex128, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexReal))
        }
        (hir::UnaryOp::Imag, out::RustType::Complex128, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexImag))
        }
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid unary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

fn lower_binary_op(
    op: hir::BinaryOp,
    left: out::RustType,
    right: out::RustType,
    result: out::RustType,
) -> Result<out::ValueOp, Diagnostic> {
    use hir::BinaryOp as Go;
    use out::RustType::{Bool, Complex128, F64, GoString, I64};
    use out::ValueOp::{Primitive, Runtime};
    let lowered = match (op, left, right, result) {
        (Go::Add, I64, I64, I64) => Primitive(PrimitiveOp::IntWrappingAdd),
        (Go::Sub, I64, I64, I64) => Primitive(PrimitiveOp::IntWrappingSub),
        (Go::Mul, I64, I64, I64) => Primitive(PrimitiveOp::IntWrappingMul),
        (Go::Div, I64, I64, I64) => Runtime(RuntimeOp::IntDiv),
        (Go::Rem, I64, I64, I64) => Runtime(RuntimeOp::IntRem),
        (Go::BitAnd, I64, I64, I64) => Primitive(PrimitiveOp::IntBitAnd),
        (Go::BitOr, I64, I64, I64) => Primitive(PrimitiveOp::IntBitOr),
        (Go::BitXor, I64, I64, I64) => Primitive(PrimitiveOp::IntBitXor),
        (Go::Shl, I64, I64, I64) => Runtime(RuntimeOp::IntShl),
        (Go::Shr, I64, I64, I64) => Runtime(RuntimeOp::IntShr),
        (Go::AndNot, I64, I64, I64) => Primitive(PrimitiveOp::IntAndNot),
        (Go::Equal, Bool, Bool, Bool) => Primitive(PrimitiveOp::BoolEqual),
        (Go::NotEqual, Bool, Bool, Bool) => Primitive(PrimitiveOp::BoolNotEqual),
        (Go::Equal, I64, I64, Bool) => Primitive(PrimitiveOp::IntEqual),
        (Go::NotEqual, I64, I64, Bool) => Primitive(PrimitiveOp::IntNotEqual),
        (Go::Less, I64, I64, Bool) => Primitive(PrimitiveOp::IntLess),
        (Go::LessEqual, I64, I64, Bool) => Primitive(PrimitiveOp::IntLessEqual),
        (Go::Greater, I64, I64, Bool) => Primitive(PrimitiveOp::IntGreater),
        (Go::GreaterEqual, I64, I64, Bool) => Primitive(PrimitiveOp::IntGreaterEqual),
        (Go::Min, I64, I64, I64) => Primitive(PrimitiveOp::IntMin),
        (Go::Max, I64, I64, I64) => Primitive(PrimitiveOp::IntMax),
        (Go::Add, F64, F64, F64) => Primitive(PrimitiveOp::FloatAdd),
        (Go::Sub, F64, F64, F64) => Primitive(PrimitiveOp::FloatSub),
        (Go::Mul, F64, F64, F64) => Primitive(PrimitiveOp::FloatMul),
        (Go::Div, F64, F64, F64) => Primitive(PrimitiveOp::FloatDiv),
        (Go::Equal, F64, F64, Bool) => Primitive(PrimitiveOp::FloatEqual),
        (Go::NotEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatNotEqual),
        (Go::Less, F64, F64, Bool) => Primitive(PrimitiveOp::FloatLess),
        (Go::LessEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatLessEqual),
        (Go::Greater, F64, F64, Bool) => Primitive(PrimitiveOp::FloatGreater),
        (Go::GreaterEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatGreaterEqual),
        (Go::Min, F64, F64, F64) => Primitive(PrimitiveOp::FloatMin),
        (Go::Max, F64, F64, F64) => Primitive(PrimitiveOp::FloatMax),
        (Go::Complex, F64, F64, Complex128) => Primitive(PrimitiveOp::ComplexFromParts),
        (Go::Add, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexAdd),
        (Go::Sub, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexSub),
        (Go::Mul, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexMul),
        (Go::Div, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexDiv),
        (Go::Equal, Complex128, Complex128, Bool) => Primitive(PrimitiveOp::ComplexEqual),
        (Go::NotEqual, Complex128, Complex128, Bool) => Primitive(PrimitiveOp::ComplexNotEqual),
        (Go::Add, GoString, GoString, GoString) => Runtime(RuntimeOp::ConcatGoStrings),
        (Go::Equal, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringEqual),
        (Go::NotEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringNotEqual),
        (Go::Less, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringLess),
        (Go::LessEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringLessEqual),
        (Go::Greater, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringGreater),
        (Go::GreaterEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringGreaterEqual),
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid binary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

fn lower_type(ty: &Ty) -> Result<out::RustType, Diagnostic> {
    let ty = ty.default_typed();
    match ty.underlying() {
        Ty::Unit => Ok(out::RustType::Unit),
        Ty::Bool => Ok(out::RustType::Bool),
        Ty::Int(IntTy::Int) => Ok(out::RustType::I64),
        Ty::Float(FloatTy::Float64) => Ok(out::RustType::F64),
        Ty::Complex(ComplexTy::Complex128) => Ok(out::RustType::Complex128),
        Ty::String => Ok(out::RustType::GoString),
        Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(out::RustType::GoSliceI64)
        }
        Ty::Slice(element)
            if element.underlying() == &Ty::Uint(crate::compiler::types::UintTy::Uint8) =>
        {
            Ok(out::RustType::GoSliceU8)
        }
        Ty::Map(key, value)
            if key.underlying() == &Ty::String && value.underlying() == &Ty::Int(IntTy::Int) =>
        {
            Ok(out::RustType::GoMapStringI64)
        }
        Ty::Pointer(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(out::RustType::GoPointerI64)
        }
        Ty::Channel(_, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(out::RustType::GoChannelI64)
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
            Ok(out::RustType::ArrayI64(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Bool => {
            Ok(out::RustType::ArrayBool(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::Float(FloatTy::Float64) => {
            Ok(out::RustType::ArrayF64(*length))
        }
        Ty::Array(length, element) if element.underlying() == &Ty::String => {
            Ok(out::RustType::ArrayGoString(*length))
        }
        Ty::Struct(fields)
            if fields
                .iter()
                .all(|field| field.ty.underlying() == &Ty::Int(IntTy::Int)) =>
        {
            Ok(out::RustType::StructI64(
                u64::try_from(fields.len()).map_err(|_| {
                    Diagnostic::backend("struct representation length does not fit u64")
                })?,
            ))
        }
        unsupported => Err(Diagnostic::backend(format!(
            "unsupported Go type reached Rust lowering: {unsupported:?}"
        ))),
    }
}

fn mir_operand_type(
    operand: &mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::RustType, Diagnostic> {
    match operand {
        mir::Operand::Read(place) => Ok(local(locals, place.local)?.ty),
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

fn lower_provenance(provenance: mir::Provenance) -> out::Provenance {
    match provenance {
        mir::Provenance::Source(source) => out::Provenance::Source(source),
        mir::Provenance::Synthetic(mir::SyntheticOrigin::NamedResultInitialization) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::NamedResultInitialization)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::PanicCleanupInitialization) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupInitialization)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::ZeroValueCall) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::ZeroValueCall)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::PanicCleanupDispatch) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupDispatch)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::ImplicitReturn) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::ImplicitReturn)
        }
    }
}
