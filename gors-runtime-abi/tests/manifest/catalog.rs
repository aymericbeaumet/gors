use std::collections::BTreeSet;

use gors_runtime_abi::{
    FloatPrimitive, IntegerKind, IntegerKindConstraint, IntegerPrimitive, IntegerRuntimeOp,
    PrimitiveOp, RuntimeAbiManifest, RuntimeOp, RuntimeType,
};

use super::{INT_DIV, INT_REM, INT_SHL, INT_SHR, manifest};

#[test]
fn current_operation_catalogs_are_complete_and_collision_free() {
    let current = RuntimeAbiManifest::current();
    assert_eq!(current.primitive_ops(), PrimitiveOp::ALL);
    assert_eq!(current.runtime_ops(), RuntimeOp::ALL);

    let symbols = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.symbol())
        .collect::<BTreeSet<_>>();
    assert_eq!(symbols.len(), RuntimeOp::ALL.len());

    let primitive_ids = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_ids.len(), PrimitiveOp::ALL.len());
    assert_eq!(
        PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingAdd,
            kind: IntegerKind::I32,
        }
        .id()
        .get(),
        143
    );
    assert_eq!(
        PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingNeg,
            kind: IntegerKind::I32,
        }
        .id()
        .get(),
        167
    );
    assert_eq!(PrimitiveOp::ALL.len(), 283);

    let primitive_names = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.name())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_names.len(), PrimitiveOp::ALL.len());

    let runtime_ids = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(runtime_ids.len(), RuntimeOp::ALL.len());
    assert_eq!(RuntimeOp::GoChannelGoStringNil.id().get(), 125);
    assert_eq!(RuntimeOp::GoChannelGoStringTryReceive.id().get(), 135);
    assert_eq!(RuntimeOp::GoChannelGoChannelI64Nil.id().get(), 136);
    assert_eq!(RuntimeOp::GoChannelGoChannelI64TryReceive.id().get(), 146);
    assert_eq!(RuntimeOp::PrintU64.id().get(), 160);
    assert_eq!(RuntimeOp::GoMapStringI64RangeKeys.id().get(), 161);
    assert_eq!(RuntimeOp::GoMapI64GoStringRangeKeys.id().get(), 171);
    assert_eq!(RuntimeOp::GoStringToSliceRunes.id().get(), 216);
    assert_eq!(RuntimeOp::GoInterfaceBoxPointerI64.id().get(), 217);
    assert_eq!(RuntimeOp::GoInterfaceUnboxPointerI64.id().get(), 218);
    assert_eq!(RuntimeOp::GoPointerI64Equal.id().get(), 219);
    assert_eq!(RuntimeOp::GoSliceI64AppendSlice.id().get(), 220);
    assert_eq!(RuntimeOp::GoSliceInterfaceAppend.id().get(), 221);
    assert_eq!(RuntimeOp::PrintF32.id().get(), 222);
    assert_eq!(RuntimeOp::try_from(222), Ok(RuntimeOp::PrintF32));
    assert_eq!(RuntimeOp::PrintF32.symbol(), "print_f32");
    assert_eq!(RuntimeOp::GoInterfaceBoxF32.id().get(), 223);
    assert_eq!(RuntimeOp::try_from(223), Ok(RuntimeOp::GoInterfaceBoxF32));
    assert_eq!(
        RuntimeOp::GoInterfaceBoxF32.symbol(),
        "go_interface_box_f32"
    );
    assert_eq!(RuntimeOp::GoInterfaceUnboxF32.id().get(), 224);
    assert_eq!(RuntimeOp::try_from(224), Ok(RuntimeOp::GoInterfaceUnboxF32));
    assert_eq!(
        RuntimeOp::GoInterfaceUnboxF32.symbol(),
        "go_interface_unbox_f32"
    );
    assert_eq!(RuntimeOp::ALL.len(), 221);

    let integer_ids = [
        [172, 173, 174, 8, 175, 176, 177, 178],
        [179, 180, 181, 9, 182, 183, 184, 185],
        [186, 187, 188, 10, 189, 190, 191, 192],
        [193, 194, 195, 11, 196, 197, 198, 199],
        [200, 201, 202, 203, 204, 205, 206, 207],
        [208, 209, 210, 211, 212, 213, 214, 215],
    ];
    for (operation, expected_ids) in IntegerRuntimeOp::ALL.iter().zip(integer_ids) {
        for (kind, expected_id) in IntegerKind::ALL.iter().zip(expected_ids) {
            let runtime_operation = RuntimeOp::Integer {
                op: *operation,
                kind: *kind,
            };
            assert_eq!(runtime_operation.id().get(), expected_id);
            assert_eq!(
                RuntimeOp::try_from(runtime_operation.id().get()),
                Ok(runtime_operation)
            );
            assert!(RuntimeOp::ALL.contains(&runtime_operation));
        }
    }
    assert_eq!(INT_DIV.symbol(), "int_div");
    assert_eq!(INT_REM.symbol(), "int_rem");
    assert_eq!(INT_SHL.symbol(), "int_shl");
    assert_eq!(INT_SHR.symbol(), "int_shr");

    let primitive_identities = PrimitiveOp::ALL
        .iter()
        .map(|operation| manifest([*operation], []).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_identities.len(), PrimitiveOp::ALL.len());

    let operation_identities = RuntimeOp::ALL
        .iter()
        .map(|operation| manifest([], [*operation]).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(operation_identities.len(), RuntimeOp::ALL.len());
}

#[test]
fn every_runtime_i64_slot_has_the_exact_semantic_integer_constraint() {
    let mut parameter_slots = 0usize;
    let mut result_slots = 0usize;

    for operation in RuntimeOp::ALL {
        for (position, parameter) in operation.signature().parameters().iter().enumerate() {
            if *parameter != RuntimeType::I64 {
                continue;
            }
            parameter_slots += 1;
            let expected = match (*operation, position) {
                (RuntimeOp::Integer { kind, .. }, 0) => IntegerKindConstraint::Exact(kind),
                (RuntimeOp::Integer { op, .. }, 1) if op.is_shift() => op.count_constraint(),
                (RuntimeOp::Integer { kind, .. }, 1) => IntegerKindConstraint::Exact(kind),
                (RuntimeOp::PrintI64, 0) => IntegerKindConstraint::Signed,
                (RuntimeOp::PrintU64, 0) => IntegerKindConstraint::Unsigned,
                (RuntimeOp::GoInterfaceBoxI64, 1) | (RuntimeOp::GoStringFromRune, 0) => {
                    IntegerKindConstraint::Any
                }
                (RuntimeOp::GoSliceI64Set, 2)
                | (RuntimeOp::GoSliceI64Append, 1)
                | (RuntimeOp::GoPointerStructI64Set, 2) => IntegerKindConstraint::Any,
                (RuntimeOp::GoSliceU8Set, 2) => IntegerKindConstraint::Exact(IntegerKind::U8),
                _ => IntegerKindConstraint::Exact(IntegerKind::I64),
            };
            assert_eq!(
                operation.integer_parameter_constraint(position),
                expected,
                "parameter {position} of {operation:?}"
            );
        }

        for (position, result) in runtime_result_components(operation.signature().result())
            .into_iter()
            .enumerate()
        {
            if result != RuntimeType::I64 {
                continue;
            }
            result_slots += 1;
            let expected = match (*operation, position) {
                (RuntimeOp::Integer { kind, .. }, 0) => IntegerKindConstraint::Exact(kind),
                (RuntimeOp::GoInterfaceUnboxI64, 0)
                | (RuntimeOp::GoPointerStructI64Get, 0)
                | (RuntimeOp::GoSliceI64Index, 0) => IntegerKindConstraint::Any,
                (RuntimeOp::GoSliceU8Index | RuntimeOp::GoStringIndex, 0) => {
                    IntegerKindConstraint::Exact(IntegerKind::U8)
                }
                (RuntimeOp::GoStringRangeRuneAt, 0) => {
                    IntegerKindConstraint::Exact(IntegerKind::I32)
                }
                _ => IntegerKindConstraint::Exact(IntegerKind::I64),
            };
            assert_eq!(
                operation.integer_result_constraint(position),
                expected,
                "result {position} of {operation:?}"
            );
        }
    }

    assert!(
        parameter_slots > 50,
        "the runtime catalog unexpectedly lost I64 parameters"
    );
    assert!(
        result_slots > 20,
        "the runtime catalog unexpectedly lost I64 results"
    );
}

#[test]
fn integer_kind_constraints_accept_only_their_declared_semantic_sets() {
    for (constraint, accepted) in [
        (
            IntegerKindConstraint::Exact(IntegerKind::I64),
            &[IntegerKind::I64][..],
        ),
        (
            IntegerKindConstraint::I64OrI32,
            &[IntegerKind::I32, IntegerKind::I64][..],
        ),
        (IntegerKindConstraint::Any, IntegerKind::ALL),
        (
            IntegerKindConstraint::Signed,
            &[
                IntegerKind::I8,
                IntegerKind::I16,
                IntegerKind::I32,
                IntegerKind::I64,
            ][..],
        ),
        (
            IntegerKindConstraint::Unsigned,
            &[
                IntegerKind::U8,
                IntegerKind::U16,
                IntegerKind::U32,
                IntegerKind::U64,
            ][..],
        ),
    ] {
        for kind in IntegerKind::ALL {
            assert_eq!(
                constraint.accepts(*kind),
                accepted.contains(kind),
                "{constraint:?} acceptance for {kind:?}"
            );
        }
    }
}

#[test]
fn primitive_signatures_are_complete_and_exact() {
    for operation in PrimitiveOp::ALL {
        let expected: (&[RuntimeType], RuntimeType) = match operation {
            PrimitiveOp::BoolNot => (&[RuntimeType::Bool], RuntimeType::Bool),
            PrimitiveOp::BoolEqual | PrimitiveOp::BoolNotEqual => {
                (&[RuntimeType::Bool, RuntimeType::Bool], RuntimeType::Bool)
            }
            PrimitiveOp::Integer { op, .. } if op.arity() == 1 => {
                (&[RuntimeType::I64], RuntimeType::I64)
            }
            PrimitiveOp::Integer { op, .. } if op.returns_bool() => {
                (&[RuntimeType::I64, RuntimeType::I64], RuntimeType::Bool)
            }
            PrimitiveOp::Integer { .. } => {
                (&[RuntimeType::I64, RuntimeType::I64], RuntimeType::I64)
            }
            PrimitiveOp::IntegerConvert { .. } => (&[RuntimeType::I64], RuntimeType::I64),
            PrimitiveOp::IntegerToFloat { .. } => (&[RuntimeType::I64], RuntimeType::F64),
            PrimitiveOp::FloatToInteger { .. } => (&[RuntimeType::F64], RuntimeType::I64),
            PrimitiveOp::FloatNeg | PrimitiveOp::FloatRound32 | PrimitiveOp::FloatWiden64 => {
                (&[RuntimeType::F64], RuntimeType::F64)
            }
            PrimitiveOp::Float32 {
                op: FloatPrimitive::Neg,
            } => (&[RuntimeType::F64], RuntimeType::F64),
            PrimitiveOp::Float32 {
                op:
                    FloatPrimitive::Equal
                    | FloatPrimitive::NotEqual
                    | FloatPrimitive::Less
                    | FloatPrimitive::LessEqual
                    | FloatPrimitive::Greater
                    | FloatPrimitive::GreaterEqual,
            } => (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::Bool),
            PrimitiveOp::Float32 {
                op:
                    FloatPrimitive::Add
                    | FloatPrimitive::Sub
                    | FloatPrimitive::Mul
                    | FloatPrimitive::Div
                    | FloatPrimitive::Min
                    | FloatPrimitive::Max,
            } => (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::F64),
            PrimitiveOp::FloatAdd
            | PrimitiveOp::FloatSub
            | PrimitiveOp::FloatMul
            | PrimitiveOp::FloatDiv
            | PrimitiveOp::FloatMin
            | PrimitiveOp::FloatMax => (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::F64),
            PrimitiveOp::FloatEqual
            | PrimitiveOp::FloatNotEqual
            | PrimitiveOp::FloatLess
            | PrimitiveOp::FloatLessEqual
            | PrimitiveOp::FloatGreater
            | PrimitiveOp::FloatGreaterEqual => {
                (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::Bool)
            }
            PrimitiveOp::ComplexNeg => (&[RuntimeType::Complex128], RuntimeType::Complex128),
            PrimitiveOp::ComplexAdd
            | PrimitiveOp::ComplexSub
            | PrimitiveOp::ComplexMul
            | PrimitiveOp::ComplexDiv => (
                &[RuntimeType::Complex128, RuntimeType::Complex128],
                RuntimeType::Complex128,
            ),
            PrimitiveOp::ComplexEqual | PrimitiveOp::ComplexNotEqual => (
                &[RuntimeType::Complex128, RuntimeType::Complex128],
                RuntimeType::Bool,
            ),
            PrimitiveOp::ComplexFromParts => (
                &[RuntimeType::F64, RuntimeType::F64],
                RuntimeType::Complex128,
            ),
            PrimitiveOp::ComplexReal
            | PrimitiveOp::ComplexImag
            | PrimitiveOp::Complex64Real
            | PrimitiveOp::Complex64Imag => (&[RuntimeType::Complex128], RuntimeType::F64),
            PrimitiveOp::Complex128ToComplex64 | PrimitiveOp::Complex64ToComplex128 => {
                (&[RuntimeType::Complex128], RuntimeType::Complex128)
            }
            PrimitiveOp::StringEqual
            | PrimitiveOp::StringNotEqual
            | PrimitiveOp::StringLess
            | PrimitiveOp::StringLessEqual
            | PrimitiveOp::StringGreater
            | PrimitiveOp::StringGreaterEqual => (
                &[RuntimeType::GoString, RuntimeType::GoString],
                RuntimeType::Bool,
            ),
        };

        assert_eq!(
            operation.signature().parameters(),
            expected.0,
            "{operation:?}"
        );
        assert_eq!(operation.signature().result(), expected.1, "{operation:?}");
    }
}

fn runtime_result_components(result: RuntimeType) -> Vec<RuntimeType> {
    use RuntimeType::{
        Bool, ByteSlice, Complex128, F64, GoChannelGoChannelI64, GoChannelGoString, GoChannelI64,
        GoChannelI64BoolTuple, GoChannelI64I64Tuple, GoInterface, GoMapI64GoString, GoMapStringI64,
        GoMapStringInterface, GoPanicPayload, GoPointerI64, GoPointerStructI64, GoSliceBool,
        GoSliceGoString, GoSliceI64, GoSliceInterface, GoSliceU8, GoString, GoStringBoolTuple,
        GoStringI64Tuple, I64, I64BoolTuple, I64I64Tuple, StaticBoolSlice, StaticByteSlice,
        StaticI64Slice, Unit,
    };
    match result {
        Unit => Vec::new(),
        I64BoolTuple => vec![I64, Bool],
        I64I64Tuple => vec![I64, I64],
        GoStringBoolTuple => vec![GoString, Bool],
        GoStringI64Tuple => vec![GoString, I64],
        GoChannelI64BoolTuple => vec![GoChannelI64, Bool],
        GoChannelI64I64Tuple => vec![GoChannelI64, I64],
        Bool
        | I64
        | GoString
        | ByteSlice
        | StaticByteSlice
        | F64
        | Complex128
        | GoSliceI64
        | StaticI64Slice
        | GoSliceU8
        | GoMapStringI64
        | GoMapI64GoString
        | GoPointerI64
        | GoChannelI64
        | GoPointerStructI64
        | GoInterface
        | StaticBoolSlice
        | GoSliceBool
        | GoSliceInterface
        | GoMapStringInterface
        | GoPanicPayload
        | GoChannelGoString
        | GoChannelGoChannelI64
        | GoSliceGoString => vec![result],
    }
}
