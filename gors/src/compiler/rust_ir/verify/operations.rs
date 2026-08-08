use super::verify_same;
use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::{RustType, ValueOp};
use gors_runtime_abi::{
    FloatKind, FloatPrimitive, IntegerKindConstraint, PrimitiveOp, RuntimeOp, RuntimeSignature,
    RuntimeType,
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
        ValueOp::Primitive(PrimitiveOp::Float32 { op }) => {
            verify_float_operation(op, FloatKind::F32, arguments, context)
        }
        ValueOp::Primitive(
            operation @ (PrimitiveOp::FloatAdd
            | PrimitiveOp::FloatSub
            | PrimitiveOp::FloatMul
            | PrimitiveOp::FloatDiv
            | PrimitiveOp::FloatNeg
            | PrimitiveOp::FloatEqual
            | PrimitiveOp::FloatNotEqual
            | PrimitiveOp::FloatLess
            | PrimitiveOp::FloatLessEqual
            | PrimitiveOp::FloatGreater
            | PrimitiveOp::FloatGreaterEqual
            | PrimitiveOp::FloatMin
            | PrimitiveOp::FloatMax),
        ) => verify_float_operation(
            legacy_float_primitive(operation)?,
            FloatKind::F64,
            arguments,
            context,
        ),
        ValueOp::Primitive(PrimitiveOp::IntegerToFloat { from, to }) => {
            verify_exact_arguments(arguments, &[RustType::Integer(from)], context)?;
            Ok(RustType::Float(to))
        }
        ValueOp::Primitive(PrimitiveOp::FloatToInteger { from, to }) => {
            verify_exact_arguments(arguments, &[RustType::Float(from)], context)?;
            Ok(RustType::Integer(to))
        }
        ValueOp::Primitive(PrimitiveOp::FloatRound32) => {
            verify_exact_arguments(arguments, &[RustType::Float(FloatKind::F64)], context)?;
            Ok(RustType::Float(FloatKind::F32))
        }
        ValueOp::Primitive(PrimitiveOp::FloatWiden64) => {
            verify_exact_arguments(arguments, &[RustType::Float(FloatKind::F32)], context)?;
            Ok(RustType::Float(FloatKind::F64))
        }
        ValueOp::Primitive(PrimitiveOp::Complex128ToComplex64) => {
            verify_exact_arguments(arguments, &[RustType::Complex(FloatKind::F64)], context)?;
            Ok(RustType::Complex(FloatKind::F32))
        }
        ValueOp::Primitive(PrimitiveOp::Complex64ToComplex128) => {
            verify_exact_arguments(arguments, &[RustType::Complex(FloatKind::F32)], context)?;
            Ok(RustType::Complex(FloatKind::F64))
        }
        ValueOp::Primitive(PrimitiveOp::Complex64Real | PrimitiveOp::Complex64Imag) => {
            verify_exact_arguments(arguments, &[RustType::Complex(FloatKind::F32)], context)?;
            Ok(RustType::Float(FloatKind::F32))
        }
        ValueOp::Primitive(PrimitiveOp::ComplexReal | PrimitiveOp::ComplexImag) => {
            verify_exact_arguments(arguments, &[RustType::Complex(FloatKind::F64)], context)?;
            Ok(RustType::Float(FloatKind::F64))
        }
        ValueOp::Primitive(PrimitiveOp::ComplexFromParts) => {
            verify_exact_arguments(
                arguments,
                &[
                    RustType::Float(FloatKind::F64),
                    RustType::Float(FloatKind::F64),
                ],
                context,
            )?;
            Ok(RustType::Complex(FloatKind::F64))
        }
        ValueOp::Primitive(
            operation @ (PrimitiveOp::ComplexAdd
            | PrimitiveOp::ComplexSub
            | PrimitiveOp::ComplexMul
            | PrimitiveOp::ComplexDiv
            | PrimitiveOp::ComplexNeg
            | PrimitiveOp::ComplexEqual
            | PrimitiveOp::ComplexNotEqual),
        ) => {
            let arity = operation.signature().parameters().len();
            let expected = vec![RustType::Complex(FloatKind::F64); arity];
            verify_exact_arguments(arguments, &expected, context)?;
            Ok(
                if matches!(
                    operation,
                    PrimitiveOp::ComplexEqual | PrimitiveOp::ComplexNotEqual
                ) {
                    RustType::Bool
                } else {
                    RustType::Complex(FloatKind::F64)
                },
            )
        }
        ValueOp::Primitive(operation) => {
            verify_operation_signature(operation.signature(), arguments, context)
        }
        ValueOp::Runtime(operation) => {
            verify_runtime_arguments(operation, arguments, context)?;
            match operation {
                RuntimeOp::Integer { kind, .. } => Ok(RustType::Integer(kind)),
                RuntimeOp::GoInterfaceUnboxF32 => Ok(RustType::Float(FloatKind::F32)),
                _ => rust_type_from_runtime(operation.signature().result(), context),
            }
        }
    }
}

fn verify_float_operation(
    operation: FloatPrimitive,
    kind: FloatKind,
    arguments: &[RustType],
    context: &str,
) -> Result<RustType, Diagnostic> {
    let expected = vec![RustType::Float(kind); operation.arity()];
    verify_exact_arguments(arguments, &expected, context)?;
    Ok(if operation.returns_bool() {
        RustType::Bool
    } else {
        RustType::Float(kind)
    })
}

fn legacy_float_primitive(operation: PrimitiveOp) -> Result<FloatPrimitive, Diagnostic> {
    Ok(match operation {
        PrimitiveOp::FloatAdd => FloatPrimitive::Add,
        PrimitiveOp::FloatSub => FloatPrimitive::Sub,
        PrimitiveOp::FloatMul => FloatPrimitive::Mul,
        PrimitiveOp::FloatDiv => FloatPrimitive::Div,
        PrimitiveOp::FloatNeg => FloatPrimitive::Neg,
        PrimitiveOp::FloatEqual => FloatPrimitive::Equal,
        PrimitiveOp::FloatNotEqual => FloatPrimitive::NotEqual,
        PrimitiveOp::FloatLess => FloatPrimitive::Less,
        PrimitiveOp::FloatLessEqual => FloatPrimitive::LessEqual,
        PrimitiveOp::FloatGreater => FloatPrimitive::Greater,
        PrimitiveOp::FloatGreaterEqual => FloatPrimitive::GreaterEqual,
        PrimitiveOp::FloatMin => FloatPrimitive::Min,
        PrimitiveOp::FloatMax => FloatPrimitive::Max,
        _ => {
            return Err(Diagnostic::backend(
                "legacy float operation selector received a non-float operation",
            ));
        }
    })
}

fn verify_exact_arguments(
    actual: &[RustType],
    expected: &[RustType],
    context: &str,
) -> Result<(), Diagnostic> {
    if actual != expected {
        return Err(Diagnostic::backend(format!(
            "Rust IR {context} has exact operands {actual:?}, expected {expected:?}"
        )));
    }
    Ok(())
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
    match operation {
        RuntimeOp::PrintF32 => {
            return verify_exact_arguments(arguments, &[RustType::Float(FloatKind::F32)], context);
        }
        RuntimeOp::PrintF64 => {
            return verify_exact_arguments(arguments, &[RustType::Float(FloatKind::F64)], context);
        }
        RuntimeOp::GoInterfaceBoxF32 => {
            return verify_exact_arguments(
                arguments,
                &[RustType::GoString, RustType::Float(FloatKind::F32)],
                context,
            );
        }
        _ => {}
    }
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
        RuntimeType::F64 => Ok(RustType::Float(FloatKind::F64)),
        RuntimeType::Complex128 => Ok(RustType::Complex(FloatKind::F64)),
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

#[cfg(test)]
mod integer_runtime_tests {
    use super::*;
    use gors_runtime_abi::{IntegerKind, IntegerRuntimeOp};

    #[test]
    fn float32_interface_unbox_has_an_exact_semantic_rvalue_result() {
        assert_eq!(
            verify_value_operation(
                ValueOp::Runtime(RuntimeOp::GoInterfaceUnboxF32),
                &[RustType::GoInterface, RustType::GoString],
                "test",
            ),
            Ok(RustType::Float(FloatKind::F32))
        );
    }

    #[test]
    fn verifier_accepts_every_exact_integer_division_and_remainder_member() {
        for kind in IntegerKind::ALL {
            let arguments = [RustType::Integer(*kind), RustType::Integer(*kind)];
            for op in [IntegerRuntimeOp::Div, IntegerRuntimeOp::Rem] {
                let operation = RuntimeOp::Integer { op, kind: *kind };
                let expected = RustType::Integer(*kind);
                let actual =
                    verify_value_operation(ValueOp::Runtime(operation), &arguments, "test");
                assert!(
                    matches!(&actual, Ok(value) if value == &expected),
                    "{actual:?}"
                );
            }
        }
    }

    #[test]
    fn verifier_accepts_every_independent_integer_shift_count_and_rejects_corruption() {
        for lhs in IntegerKind::ALL {
            for count in IntegerKind::ALL {
                let op = if count.is_signed() {
                    IntegerRuntimeOp::ShlSigned
                } else {
                    IntegerRuntimeOp::ShlUnsigned
                };
                let operation = RuntimeOp::Integer { op, kind: *lhs };
                let arguments = [RustType::Integer(*lhs), RustType::Integer(*count)];
                let expected = RustType::Integer(*lhs);
                let actual =
                    verify_value_operation(ValueOp::Runtime(operation), &arguments, "test");
                assert!(
                    matches!(&actual, Ok(value) if value == &expected),
                    "{actual:?}"
                );

                let wrong_op = if count.is_signed() {
                    IntegerRuntimeOp::ShlUnsigned
                } else {
                    IntegerRuntimeOp::ShlSigned
                };
                let corrupted = verify_value_operation(
                    ValueOp::Runtime(RuntimeOp::Integer {
                        op: wrong_op,
                        kind: *lhs,
                    }),
                    &arguments,
                    "corrupt shift",
                );
                assert!(
                    matches!(&corrupted, Err(error) if error.message.contains("requires")),
                    "{corrupted:?}"
                );
            }
        }
    }
}
