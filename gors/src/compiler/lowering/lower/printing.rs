//! Ordered lowering for Go's diagnostic `print` and `println` builtins.

use super::control::finish_terminator;
use super::{lower_operand, mir_operand_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use gors_runtime_abi::RuntimeOp;

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_print_call(
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
            out::RustType::F64 => RuntimeOp::PrintF64,
            out::RustType::I64 => RuntimeOp::PrintI64,
            out::RustType::GoString => RuntimeOp::PrintGoString,
            out::RustType::Complex128
            | out::RustType::ArrayI64(_)
            | out::RustType::ArrayBool(_)
            | out::RustType::ArrayF64(_)
            | out::RustType::ArrayGoString(_)
            | out::RustType::ArrayGoPointerStructI64(_)
            | out::RustType::Struct(_)
            | out::RustType::StructI64(_)
            | out::RustType::GoSliceI64
            | out::RustType::GoSliceU8
            | out::RustType::GoSliceBool
            | out::RustType::GoSliceInterface
            | out::RustType::GoSliceGoString
            | out::RustType::GoMapStringI64
            | out::RustType::GoMapStringInterface
            | out::RustType::GoPointerI64
            | out::RustType::GoPointerStructI64
            | out::RustType::GoInterface
            | out::RustType::GoChannelI64
            | out::RustType::GoChannelGoString
            | out::RustType::GoChannelGoChannelI64 => {
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
            provenance,
            statements: Vec::new(),
            terminator: finish_terminator(
                out::TerminatorKind::Call {
                    target: out::CallTarget::Runtime(operation),
                    args,
                    destinations: Vec::new(),
                    next: call_next,
                },
                provenance,
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
