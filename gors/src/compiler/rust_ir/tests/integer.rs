use super::*;

#[test]
fn verifier_rejects_integer_primitive_kind_and_conversion_corruption() {
    let mut mislabeled_add =
        lower("package main\nfunc add(left int8, right int8) int8 { return left + right }\n");
    *binary_value_op_mut(
        &mut mislabeled_add,
        ValueOp::Primitive(PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingAdd,
            kind: IntegerKind::I8,
        }),
    ) = ValueOp::Primitive(PrimitiveOp::Integer {
        op: IntegerPrimitive::WrappingAdd,
        kind: IntegerKind::U8,
    });
    refresh_test_effects(&mut mislabeled_add);
    let error = mislabeled_add.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("does not preserve U8 integer operands"),
        "{error:?}"
    );

    let source = "package main\nfunc convert(value int8) uint16 { return uint16(value) }\n";
    for replacement in [
        PrimitiveOp::IntegerConvert {
            from: IntegerKind::U8,
            to: IntegerKind::U16,
        },
        PrimitiveOp::IntegerConvert {
            from: IntegerKind::I8,
            to: IntegerKind::U8,
        },
    ] {
        let mut conversion = lower(source);
        *unary_value_op_mut(
            &mut conversion,
            ValueOp::Primitive(PrimitiveOp::IntegerConvert {
                from: IntegerKind::I8,
                to: IntegerKind::U16,
            }),
        ) = ValueOp::Primitive(replacement);
        refresh_test_effects(&mut conversion);
        let error = conversion.verify().unwrap_err();
        assert!(
            error.message.contains("conversion types")
                || error.message.contains("assignment type mismatch"),
            "{replacement:?} produced {error:?}"
        );
    }
}

#[test]
fn verifier_rejects_noncanonical_integer_constants_and_static_arrays() {
    let mut scalar = lower("package main\nfunc main() { value := int8(1); print(value) }\n");
    let bits = integer_constant_bits_mut(&mut scalar, IntegerKind::I8);
    *bits = 128;
    let error = scalar.verify().unwrap_err();
    assert!(
        error.message.contains("non-canonical I8 carrier"),
        "{error:?}"
    );

    let mut array =
        lower("package main\nfunc main() { values := [1]int{128}; println(values[0]) }\n");
    let (kind, values) = static_integer_array_parts_mut(&mut array);
    *kind = IntegerKind::I8;
    assert_eq!(values.as_slice(), [128]);
    let error = array.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("static integer array has a non-canonical I8 element"),
        "{error:?}"
    );
}

#[test]
fn verifier_enforces_signed_unsigned_print_and_exact_integer_runtime_operations() {
    for (source, expected, replacement) in [
        (
            "package main\nfunc main() { print(int8(-1)) }\n",
            RuntimeOp::PrintI64,
            RuntimeOp::PrintU64,
        ),
        (
            "package main\nfunc main() { print(uint64(18446744073709551615)) }\n",
            RuntimeOp::PrintU64,
            RuntimeOp::PrintI64,
        ),
    ] {
        let mut file = lower(source);
        *runtime_call_target_mut(&mut file, expected) = replacement;
        refresh_test_effects(&mut file);
        let error = file.verify().unwrap_err();
        assert!(
            error.message.contains("runtime call argument 0 requires"),
            "{expected:?} -> {replacement:?} produced {error:?}"
        );
    }

    let mut division =
        lower("package main\nfunc add(left uint64, right uint64) uint64 { return left + right }\n");
    *binary_value_op_mut(
        &mut division,
        ValueOp::Primitive(PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingAdd,
            kind: IntegerKind::U64,
        }),
    ) = ValueOp::Runtime(INT_DIV);
    refresh_test_effects(&mut division);
    let error = division.verify().unwrap_err();
    assert!(error.message.contains("requires Exact(I64)"), "{error:?}");
}

#[test]
fn shift_runtime_effects_follow_the_independent_count_kind() {
    let signed_operation = RuntimeOp::Integer {
        op: IntegerRuntimeOp::ShlSigned,
        kind: IntegerKind::U8,
    };
    let signed = lower(
        "package main\nfunc shift(value uint8, count int8) uint8 { return value << count }\n",
    );
    let signed_shift = binary_rvalue(&signed, ValueOp::Runtime(signed_operation));
    assert!(signed_shift.effects.may_panic);
    assert_ne!(signed_shift.panic, PanicEdge::None);

    let unsigned_operation = RuntimeOp::Integer {
        op: IntegerRuntimeOp::ShlUnsigned,
        kind: IntegerKind::U8,
    };
    let mut unsigned = lower(
        "package main\nfunc shift(value uint8, count uint64) uint8 { return value << count }\n",
    );
    let unsigned_shift = binary_rvalue_mut(&mut unsigned, ValueOp::Runtime(unsigned_operation));
    assert!(!unsigned_shift.effects.may_panic);
    assert_eq!(unsigned_shift.panic, PanicEdge::None);
    unsigned_shift.effects.may_panic = true;

    let error = unsigned.verify().unwrap_err();
    assert!(error.message.contains("effect mismatch"), "{error:?}");
}

#[test]
fn verifier_rejects_byte_and_rune_runtime_result_destination_swaps() {
    for (source, operation, replacement) in [
        (
            "package main\nfunc byteAt(value string) byte { return value[0] }\n",
            RuntimeOp::GoStringIndex,
            RustType::Integer(IntegerKind::I64),
        ),
        (
            "package main\nfunc main() { for _, value := range \"x\" { print(value) } }\n",
            RuntimeOp::GoStringRangeRuneAt,
            RustType::Integer(IntegerKind::U8),
        ),
    ] {
        let mut file = lower(source);
        let (function, destination) = runtime_call_destination(&file, operation);
        file.functions[function].locals[destination.0 as usize].ty = replacement.clone();
        let error = file.verify().unwrap_err();
        assert!(
            error
                .message
                .contains("runtime call destination 0 requires"),
            "{operation:?} -> {replacement:?} produced {error:?}"
        );
    }
}

#[test]
fn verifier_requires_go_int_indices_for_scalar_arrays() {
    for source in [
        "package main\nfunc main() { values := [1]bool{true}; index := 0; _ = values[index] }\n",
        "package main\nfunc main() { values := [1]bool{true}; index := 0; values[index] = false }\n",
    ] {
        let mut file = lower(source);
        let index = scalar_array_index_operand_mut(&mut file)
            .expect("scalar array read or update index operand");
        *index = Operand::Constant(Constant::Integer {
            kind: IntegerKind::U64,
            bits: -1,
        });
        let error = file.verify().unwrap_err();
        assert!(
            error.message.contains("index does not use Go int"),
            "{error:?}"
        );
    }
}

#[test]
fn verifier_rejects_cross_kind_integer_array_equality() {
    let mut file = lower(
        "package main\nfunc main() { left := [1]byte{1}; right := [1]byte{1}; _ = left == right }\n",
    );
    *aggregate_equal_right_mut(&mut file) = Operand::Constant(Constant::StaticIntegerArray {
        kind: IntegerKind::I16,
        values: vec![1],
    });
    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("invalid Rust IR aggregate equality types"),
        "{error:?}"
    );
}

fn unary_value_op_mut(file: &mut File, expected: ValueOp) -> &mut ValueOp {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Unary { op, .. } if *op == expected => Some(op),
            _ => None,
        })
        .unwrap()
}

fn integer_constant_bits_mut(file: &mut File, expected: IntegerKind) -> &mut i64 {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Use(Operand::Constant(constant)) => match constant {
                Constant::Integer { kind, bits } if *kind == expected => Some(bits),
                _ => None,
            },
            _ => None,
        })
        .unwrap()
}

fn static_integer_array_parts_mut(file: &mut File) -> (&mut IntegerKind, &mut Vec<i64>) {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Use(Operand::Constant(Constant::StaticIntegerArray { kind, values })) => {
                Some((kind, values))
            }
            _ => None,
        })
        .unwrap()
}

fn runtime_call_destination(file: &File, expected: RuntimeOp) -> (usize, LocalId) {
    file.functions
        .iter()
        .enumerate()
        .find_map(|(function_index, function)| {
            function
                .blocks
                .iter()
                .find_map(|block| match &block.terminator.kind {
                    TerminatorKind::Call {
                        target: CallTarget::Runtime(operation),
                        destinations,
                        ..
                    } if *operation == expected => destinations
                        .first()
                        .map(|destination| (function_index, destination.local)),
                    _ => None,
                })
        })
        .unwrap()
}

fn scalar_array_index_operand_mut(file: &mut File) -> Option<&mut Operand> {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::ArrayIndex { index, .. } | RvalueKind::ArraySet { index, .. } => {
                Some(index)
            }
            _ => None,
        })
}

fn aggregate_equal_right_mut(file: &mut File) -> &mut Operand {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::AggregateEqualInteger { right, .. } => Some(right),
            _ => None,
        })
        .unwrap()
}
