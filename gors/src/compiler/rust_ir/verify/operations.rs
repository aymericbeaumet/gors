use super::verify_same;
use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::{RustType, ValueOp};
use gors_runtime_abi::{
    IntegerKind, IntegerKindConstraint, PrimitiveOp, RuntimeOp, RuntimeSignature, RuntimeType,
};

pub(super) fn verify_value_operation(
    operation: ValueOp,
    arguments: &[RustType],
    context: &str,
) -> Result<RustType, Diagnostic> {
    match operation {
        ValueOp::Primitive(PrimitiveOp::Integer { op, kind }) => {
            if arguments.len() != op.arity()
                || arguments
                    .iter()
                    .any(|argument| *argument != RustType::Integer(kind))
            {
                return Err(Diagnostic::backend(format!(
                    "Rust IR {context} does not preserve {kind:?} integer operands"
                )));
            }
            Ok(if op.returns_bool() {
                RustType::Bool
            } else {
                RustType::Integer(kind)
            })
        }
        ValueOp::Primitive(PrimitiveOp::IntegerConvert { from, to }) => {
            if arguments != [RustType::Integer(from)] {
                return Err(Diagnostic::backend(format!(
                    "Rust IR {context} does not preserve {from:?}-to-{to:?} conversion types"
                )));
            }
            Ok(RustType::Integer(to))
        }
        ValueOp::Primitive(operation) => {
            verify_operation_signature(operation.signature(), arguments, context)
        }
        ValueOp::Runtime(operation) => {
            verify_runtime_arguments(operation, arguments, context)?;
            match operation {
                RuntimeOp::IntDiv | RuntimeOp::IntRem | RuntimeOp::IntShl | RuntimeOp::IntShr => {
                    Ok(RustType::Integer(IntegerKind::I64))
                }
                _ => rust_type_from_runtime(operation.signature().result(), context),
            }
        }
    }
}

fn verify_operation_signature(
    signature: RuntimeSignature,
    arguments: &[RustType],
    context: &str,
) -> Result<RustType, Diagnostic> {
    verify_operation_arguments(signature, arguments, context)?;
    rust_type_from_runtime(signature.result(), context)
}

fn verify_operation_arguments(
    signature: RuntimeSignature,
    arguments: &[RustType],
    context: &str,
) -> Result<(), Diagnostic> {
    if arguments.len() != signature.parameters().len() {
        return Err(Diagnostic::backend(format!(
            "Rust IR {context} has {} arguments but its ABI signature requires {}",
            arguments.len(),
            signature.parameters().len()
        )));
    }
    for (position, (actual, expected)) in arguments.iter().zip(signature.parameters()).enumerate() {
        let expected = rust_type_from_runtime(*expected, context)?;
        verify_same(
            actual.clone(),
            expected,
            &format!("{context} argument {position}"),
        )?;
    }
    Ok(())
}

pub(super) fn verify_runtime_arguments(
    operation: RuntimeOp,
    arguments: &[RustType],
    context: &str,
) -> Result<(), Diagnostic> {
    let signature = operation.signature();
    if arguments.len() != signature.parameters().len() {
        return Err(Diagnostic::backend(format!(
            "Rust IR {context} has {} arguments but its ABI signature requires {}",
            arguments.len(),
            signature.parameters().len()
        )));
    }
    for (position, (actual, expected)) in arguments.iter().zip(signature.parameters()).enumerate() {
        verify_runtime_type(
            actual,
            *expected,
            (*expected == RuntimeType::I64)
                .then(|| operation.integer_parameter_constraint(position)),
            &format!("{context} argument {position}"),
        )?;
    }
    Ok(())
}

pub(super) fn verify_runtime_type(
    actual: &RustType,
    expected: RuntimeType,
    integer: Option<IntegerKindConstraint>,
    context: &str,
) -> Result<(), Diagnostic> {
    if expected == RuntimeType::I64 {
        let Some(constraint) = integer else {
            return Err(Diagnostic::backend(format!(
                "Rust IR {context} is missing an ABI integer-kind constraint"
            )));
        };
        return match actual {
            RustType::Integer(kind) if constraint.accepts(*kind) => Ok(()),
            _ => Err(Diagnostic::backend(format!(
                "Rust IR {context} requires {constraint:?}, found {actual:?}"
            ))),
        };
    }
    verify_same(
        actual.clone(),
        rust_type_from_runtime(expected, context)?,
        context,
    )
}

pub(super) fn runtime_result_parts(
    ty: RuntimeType,
    context: &str,
) -> Result<Vec<RuntimeType>, Diagnostic> {
    match ty {
        RuntimeType::Unit => Ok(Vec::new()),
        RuntimeType::I64BoolTuple => Ok(vec![RuntimeType::I64, RuntimeType::Bool]),
        RuntimeType::I64I64Tuple => Ok(vec![RuntimeType::I64, RuntimeType::I64]),
        RuntimeType::GoStringBoolTuple => Ok(vec![RuntimeType::GoString, RuntimeType::Bool]),
        RuntimeType::GoStringI64Tuple => Ok(vec![RuntimeType::GoString, RuntimeType::I64]),
        RuntimeType::GoChannelI64BoolTuple => {
            Ok(vec![RuntimeType::GoChannelI64, RuntimeType::Bool])
        }
        RuntimeType::GoChannelI64I64Tuple => Ok(vec![RuntimeType::GoChannelI64, RuntimeType::I64]),
        RuntimeType::ByteSlice
        | RuntimeType::StaticByteSlice
        | RuntimeType::StaticI64Slice
        | RuntimeType::StaticBoolSlice
        | RuntimeType::GoPanicPayload => Err(Diagnostic::backend(format!(
            "Rust IR {context} requires ABI-only result type {ty:?}"
        ))),
        ty => Ok(vec![ty]),
    }
}

fn rust_type_from_runtime(ty: RuntimeType, context: &str) -> Result<RustType, Diagnostic> {
    match ty {
        RuntimeType::Unit => Ok(RustType::Unit),
        RuntimeType::Bool => Ok(RustType::Bool),
        RuntimeType::I64 => Err(Diagnostic::backend(format!(
            "Rust IR {context} requires an explicit Go integer kind"
        ))),
        RuntimeType::F64 => Ok(RustType::F64),
        RuntimeType::Complex128 => Ok(RustType::Complex128),
        RuntimeType::GoString => Ok(RustType::GoString),
        RuntimeType::GoSliceI64 => Ok(RustType::GoSliceI64),
        RuntimeType::GoSliceU8 => Ok(RustType::GoSliceU8),
        RuntimeType::GoSliceBool => Ok(RustType::GoSliceBool),
        RuntimeType::GoSliceInterface => Ok(RustType::GoSliceInterface),
        RuntimeType::GoSliceGoString => Ok(RustType::GoSliceGoString),
        RuntimeType::GoMapStringI64 => Ok(RustType::GoMapStringI64),
        RuntimeType::GoMapI64GoString => Ok(RustType::GoMapI64GoString),
        RuntimeType::GoMapStringInterface => Ok(RustType::GoMapStringInterface),
        RuntimeType::GoPointerI64 => Ok(RustType::GoPointerI64),
        RuntimeType::GoPointerStructI64 => Ok(RustType::GoPointerStructI64),
        RuntimeType::GoInterface => Ok(RustType::GoInterface),
        RuntimeType::GoChannelI64 => Ok(RustType::GoChannelI64),
        RuntimeType::GoChannelGoString => Ok(RustType::GoChannelGoString),
        RuntimeType::GoChannelGoChannelI64 => Ok(RustType::GoChannelGoChannelI64),
        RuntimeType::ByteSlice
        | RuntimeType::StaticByteSlice
        | RuntimeType::StaticI64Slice
        | RuntimeType::StaticBoolSlice
        | RuntimeType::I64BoolTuple
        | RuntimeType::I64I64Tuple
        | RuntimeType::GoStringBoolTuple
        | RuntimeType::GoStringI64Tuple
        | RuntimeType::GoChannelI64BoolTuple
        | RuntimeType::GoChannelI64I64Tuple
        | RuntimeType::GoPanicPayload => Err(Diagnostic::backend(format!(
            "Rust IR {context} requires ABI-only operand type {ty:?}"
        ))),
    }
}
