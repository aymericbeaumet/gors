use gors_runtime_abi::{
    CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, FloatKind, FloatPrimitive,
    GoSemanticModel, IntegerKind, IntegerPrimitive, PrimitiveOp, RuntimeAbiManifest, RuntimeType,
};

const LEGACY_PRIMITIVE_COUNT: usize = 233;

#[test]
fn legacy_primitive_catalog_keeps_its_exact_order_and_canonical_bytes() {
    let legacy = legacy_primitive_catalog();
    assert_eq!(legacy.len(), LEGACY_PRIMITIVE_COUNT);
    assert_eq!(
        PrimitiveOp::ALL.get(..LEGACY_PRIMITIVE_COUNT),
        Some(legacy.as_slice()),
    );

    let manifest = RuntimeAbiManifest::new(
        CURRENT_MANIFEST_SCHEMA,
        ContractVersion::new(2, 29, 0),
        GoSemanticModel::new(DataWidth::Bits64),
        legacy,
        [],
    );
    assert_eq!(
        manifest.identity().to_string(),
        "1443988a9a897e3aeb236b7f12f3f5a703fc8f716bdf595c70ff25ccbd1d6fe8",
        "a pre-2.30 primitive ID, name, signature, or encoding changed",
    );
}

#[test]
fn float_selectors_preserve_float64_and_append_float32() {
    let legacy_float64 = [
        PrimitiveOp::FloatAdd,
        PrimitiveOp::FloatSub,
        PrimitiveOp::FloatMul,
        PrimitiveOp::FloatDiv,
        PrimitiveOp::FloatNeg,
        PrimitiveOp::FloatEqual,
        PrimitiveOp::FloatNotEqual,
        PrimitiveOp::FloatLess,
        PrimitiveOp::FloatLessEqual,
        PrimitiveOp::FloatGreater,
        PrimitiveOp::FloatGreaterEqual,
        PrimitiveOp::FloatMin,
        PrimitiveOp::FloatMax,
    ];

    assert_eq!(FloatKind::ALL, &[FloatKind::F32, FloatKind::F64]);
    assert_eq!(FloatKind::F32.bits(), 32);
    assert_eq!(FloatKind::F64.bits(), 64);
    assert_eq!(FloatKind::F32.name(), "float32");
    assert_eq!(FloatKind::F64.name(), "float64");

    for (index, (primitive, legacy)) in
        (0_u16..).zip(FloatPrimitive::ALL.iter().copied().zip(legacy_float64))
    {
        assert_eq!(
            PrimitiveOp::float(primitive, FloatKind::F64),
            legacy,
            "float64 selector changed at {primitive:?}",
        );

        let float32 = PrimitiveOp::float(primitive, FloatKind::F32);
        assert_eq!(float32, PrimitiveOp::Float32 { op: primitive });
        assert_eq!(float32.id().get(), 253 + index);
        assert_eq!(
            float32.name(),
            format!("float32-{}", float_primitive_name(primitive)),
        );
        assert_eq!(
            PrimitiveOp::ALL.get(LEGACY_PRIMITIVE_COUNT + usize::from(index)),
            Some(&float32),
        );
    }
}

#[test]
fn numeric_conversion_catalog_is_complete_and_row_major() {
    let mut index = 0_u16;
    for from in IntegerKind::ALL {
        for to in FloatKind::ALL {
            let operation = PrimitiveOp::IntegerToFloat {
                from: *from,
                to: *to,
            };
            assert_eq!(operation.id().get(), 266 + index);
            assert_eq!(
                operation.name(),
                format!("{}-to-{}", integer_kind_name(*from), to.name())
            );
            assert_eq!(operation.signature().parameters(), &[RuntimeType::I64]);
            assert_eq!(operation.signature().result(), RuntimeType::F64);
            assert_eq!(
                PrimitiveOp::ALL.get(usize::from(266 + index - 20)),
                Some(&operation),
            );
            index += 1;
        }
    }
    assert_eq!(index, 16);

    index = 0;
    for from in FloatKind::ALL {
        for to in IntegerKind::ALL {
            let operation = PrimitiveOp::FloatToInteger {
                from: *from,
                to: *to,
            };
            assert_eq!(operation.id().get(), 282 + index);
            assert_eq!(
                operation.name(),
                format!("{}-to-{}", from.name(), integer_kind_name(*to))
            );
            assert_eq!(operation.signature().parameters(), &[RuntimeType::F64]);
            assert_eq!(operation.signature().result(), RuntimeType::I64);
            assert_eq!(
                PrimitiveOp::ALL.get(usize::from(282 + index - 20)),
                Some(&operation),
            );
            index += 1;
        }
    }
    assert_eq!(index, 16);
}

#[test]
fn width_conversions_have_exact_appended_contracts() {
    assert_eq!(
        PrimitiveOp::float_conversion(FloatKind::F64, FloatKind::F32),
        Some(PrimitiveOp::FloatRound32),
    );
    assert_eq!(
        PrimitiveOp::float_conversion(FloatKind::F32, FloatKind::F64),
        Some(PrimitiveOp::FloatWiden64),
    );
    assert_eq!(
        PrimitiveOp::float_conversion(FloatKind::F32, FloatKind::F32),
        None,
    );
    assert_eq!(
        PrimitiveOp::float_conversion(FloatKind::F64, FloatKind::F64),
        None,
    );

    let appended = [
        (PrimitiveOp::FloatWiden64, 298, "float32-to-float64"),
        (
            PrimitiveOp::Complex128ToComplex64,
            299,
            "complex128-to-complex64",
        ),
        (
            PrimitiveOp::Complex64ToComplex128,
            300,
            "complex64-to-complex128",
        ),
        (PrimitiveOp::Complex64Real, 301, "complex64-real"),
        (PrimitiveOp::Complex64Imag, 302, "complex64-imag"),
    ];
    for (operation, id, name) in appended {
        assert_eq!(operation.id().get(), id);
        assert_eq!(operation.name(), name);
        assert_eq!(PrimitiveOp::ALL.get(usize::from(id - 20)), Some(&operation),);
    }

    assert_eq!(
        PrimitiveOp::complex_conversion(FloatKind::F64, FloatKind::F32),
        Some(PrimitiveOp::Complex128ToComplex64),
    );
    assert_eq!(
        PrimitiveOp::complex_conversion(FloatKind::F32, FloatKind::F64),
        Some(PrimitiveOp::Complex64ToComplex128),
    );
    assert_eq!(
        PrimitiveOp::complex_conversion(FloatKind::F32, FloatKind::F32),
        None,
    );
    assert_eq!(
        PrimitiveOp::complex_conversion(FloatKind::F64, FloatKind::F64),
        None,
    );
    assert_eq!(
        PrimitiveOp::complex_real(FloatKind::F32),
        PrimitiveOp::Complex64Real,
    );
    assert_eq!(
        PrimitiveOp::complex_real(FloatKind::F64),
        PrimitiveOp::ComplexReal,
    );
    assert_eq!(
        PrimitiveOp::complex_imag(FloatKind::F32),
        PrimitiveOp::Complex64Imag,
    );
    assert_eq!(
        PrimitiveOp::complex_imag(FloatKind::F64),
        PrimitiveOp::ComplexImag,
    );
}

fn legacy_primitive_catalog() -> Vec<PrimitiveOp> {
    let mut operations = vec![
        PrimitiveOp::BoolNot,
        PrimitiveOp::BoolEqual,
        PrimitiveOp::BoolNotEqual,
        PrimitiveOp::StringEqual,
        PrimitiveOp::StringNotEqual,
        PrimitiveOp::StringLess,
        PrimitiveOp::StringLessEqual,
        PrimitiveOp::StringGreater,
        PrimitiveOp::StringGreaterEqual,
        PrimitiveOp::FloatAdd,
        PrimitiveOp::FloatSub,
        PrimitiveOp::FloatMul,
        PrimitiveOp::FloatDiv,
        PrimitiveOp::FloatNeg,
        PrimitiveOp::FloatEqual,
        PrimitiveOp::FloatNotEqual,
        PrimitiveOp::FloatLess,
        PrimitiveOp::FloatLessEqual,
        PrimitiveOp::FloatGreater,
        PrimitiveOp::FloatGreaterEqual,
        PrimitiveOp::ComplexAdd,
        PrimitiveOp::ComplexSub,
        PrimitiveOp::ComplexMul,
        PrimitiveOp::ComplexDiv,
        PrimitiveOp::ComplexNeg,
        PrimitiveOp::ComplexEqual,
        PrimitiveOp::ComplexNotEqual,
        PrimitiveOp::FloatMin,
        PrimitiveOp::FloatMax,
        PrimitiveOp::ComplexFromParts,
        PrimitiveOp::ComplexReal,
        PrimitiveOp::ComplexImag,
        PrimitiveOp::FloatRound32,
    ];
    for primitive in IntegerPrimitive::ALL {
        for kind in IntegerKind::ALL {
            operations.push(PrimitiveOp::Integer {
                op: *primitive,
                kind: *kind,
            });
        }
    }
    for from in IntegerKind::ALL {
        for to in IntegerKind::ALL {
            operations.push(PrimitiveOp::IntegerConvert {
                from: *from,
                to: *to,
            });
        }
    }
    operations
}

const fn float_primitive_name(operation: FloatPrimitive) -> &'static str {
    match operation {
        FloatPrimitive::Add => "add",
        FloatPrimitive::Sub => "sub",
        FloatPrimitive::Mul => "mul",
        FloatPrimitive::Div => "div",
        FloatPrimitive::Neg => "neg",
        FloatPrimitive::Equal => "equal",
        FloatPrimitive::NotEqual => "not-equal",
        FloatPrimitive::Less => "less",
        FloatPrimitive::LessEqual => "less-equal",
        FloatPrimitive::Greater => "greater",
        FloatPrimitive::GreaterEqual => "greater-equal",
        FloatPrimitive::Min => "min",
        FloatPrimitive::Max => "max",
    }
}

const fn integer_kind_name(kind: IntegerKind) -> &'static str {
    match kind {
        IntegerKind::I8 => "i8",
        IntegerKind::I16 => "i16",
        IntegerKind::I32 => "i32",
        IntegerKind::I64 => "i64",
        IntegerKind::U8 => "u8",
        IntegerKind::U16 => "u16",
        IntegerKind::U32 => "u32",
        IntegerKind::U64 => "u64",
    }
}
