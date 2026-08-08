use super::*;
use gors_runtime_abi::{FloatKind, FloatPrimitive};

#[test]
fn lowering_retains_exact_float_and_complex_widths_and_operations() {
    let file = lower(
        r#"
            package main
            func widths(f32 float32, f64 float64, i int16, c128 complex128, c64 complex64) (float32, float64, float32, float64, float32, int16, complex64, complex128, float32, float32) {
                return f32 + f32, f64 + f64, float32(f64), float64(f32), float32(i), int16(f32), complex64(c128), complex128(c64), real(c64), imag(c64)
            }
            func main() { println(float32(0.1), float64(0.1)) }
        "#,
    );

    let types = file
        .functions
        .iter()
        .flat_map(|function| &function.locals)
        .map(|local| local.ty.clone())
        .collect::<Vec<_>>();
    assert!(
        types.contains(&RustType::Float(FloatKind::F32)),
        "{types:?}"
    );
    assert!(
        types.contains(&RustType::Float(FloatKind::F64)),
        "{types:?}"
    );
    assert!(
        types.contains(&RustType::Complex(FloatKind::F32)),
        "{types:?}"
    );
    assert!(
        types.contains(&RustType::Complex(FloatKind::F64)),
        "{types:?}"
    );

    let operations = all_rvalues(&file)
        .filter_map(|rvalue| match rvalue.kind {
            RvalueKind::Unary { op, .. } | RvalueKind::Binary { op, .. } => Some(op),
            _ => None,
        })
        .collect::<Vec<_>>();
    for operation in [
        PrimitiveOp::float(FloatPrimitive::Add, FloatKind::F32),
        PrimitiveOp::float(FloatPrimitive::Add, FloatKind::F64),
        PrimitiveOp::FloatRound32,
        PrimitiveOp::FloatWiden64,
        PrimitiveOp::IntegerToFloat {
            from: IntegerKind::I16,
            to: FloatKind::F32,
        },
        PrimitiveOp::FloatToInteger {
            from: FloatKind::F32,
            to: IntegerKind::I16,
        },
        PrimitiveOp::Complex128ToComplex64,
        PrimitiveOp::Complex64ToComplex128,
        PrimitiveOp::Complex64Real,
        PrimitiveOp::Complex64Imag,
    ] {
        assert!(
            operations.contains(&ValueOp::Primitive(operation)),
            "missing {operation:?} in {operations:?}"
        );
    }
    let requirement = file.verify().expect("exact numeric widths must verify");
    assert!(requirement.contains(RuntimeOp::PrintF32));
    assert!(requirement.contains(RuntimeOp::PrintF64));
}

#[test]
fn verifier_rejects_float_width_operation_and_print_corruption() {
    let source = r#"
        package main
        func add(left float32, right float32) float32 { return left + right }
        func main() { println(add(float32(0.1), float32(0.2))) }
    "#;

    let mut wrong_add = lower(source);
    *binary_value_op_mut(
        &mut wrong_add,
        ValueOp::Primitive(PrimitiveOp::float(FloatPrimitive::Add, FloatKind::F32)),
    ) = ValueOp::Primitive(PrimitiveOp::FloatAdd);
    let error = wrong_add.verify().unwrap_err();
    assert!(error.message.contains("exact operands"), "{error:?}");

    let mut wrong_print = lower(source);
    *runtime_call_target_mut(&mut wrong_print, RuntimeOp::PrintF32) = RuntimeOp::PrintF64;
    refresh_test_effects(&mut wrong_print);
    let error = wrong_print.verify().unwrap_err();
    assert!(error.message.contains("exact operands"), "{error:?}");
}

#[test]
fn verifier_rejects_noncanonical_float32_constant_carriers() {
    let mut file = lower("package main\nfunc main() { print(float32(0.1)) }\n");
    let constant = file
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Use(Operand::Constant(Constant::Float {
                kind: FloatKind::F32,
                bits,
            })) => Some(bits),
            _ => None,
        })
        .expect("float32 constant carrier");
    *constant = 0.1_f64.to_bits();

    let error = file.verify().unwrap_err();
    assert!(
        error.message.contains("non-canonical float32 carrier"),
        "{error:?}"
    );
}

#[test]
fn float32_interface_payloads_retain_exact_rust_ir_width() {
    let file = lower(
        r#"
            package main
            type Narrow float32
            func main() {
                var boxed any = float32(0.1)
                value, ok := boxed.(float32)
                if !ok { panic("float32 assertion failed") }
                var named any = Narrow(0.2)
                namedValue, namedOK := named.(Narrow)
                if !namedOK { panic("named float32 assertion failed") }
                print(value)
                print(namedValue)
            }
        "#,
    );
    let requirement = file.verify().expect("float32 interface path must verify");
    for operation in [
        RuntimeOp::GoInterfaceBoxF32,
        RuntimeOp::GoInterfaceUnboxF32,
        RuntimeOp::PrintF32,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }
    assert!(
        file.functions
            .iter()
            .flat_map(|function| &function.locals)
            .any(|local| local.ty == RustType::Float(FloatKind::F32))
    );
}

#[test]
fn verifier_rejects_float_interface_operation_width_corruption() {
    let source = r#"
        package main
        func main() {
            var boxed any = float32(0.1)
            value, ok := boxed.(float32)
            if !ok { panic("float32 assertion failed") }
            print(value)
        }
    "#;

    for (operation, replacement) in [
        (RuntimeOp::GoInterfaceBoxF32, RuntimeOp::GoInterfaceBoxF64),
        (
            RuntimeOp::GoInterfaceUnboxF32,
            RuntimeOp::GoInterfaceUnboxF64,
        ),
    ] {
        let mut file = lower(source);
        *runtime_call_target_mut(&mut file, operation) = replacement;
        refresh_test_effects(&mut file);

        let error = file.verify().unwrap_err();
        assert!(
            error.message.contains("runtime call")
                && (error.message.contains("type mismatch")
                    || error.message.contains("exact operands")),
            "{operation:?}: {error:?}"
        );
    }

    let mut wrong_destination = lower(source);
    let destination =
        match &runtime_call_terminator_mut(&mut wrong_destination, RuntimeOp::GoInterfaceUnboxF32)
            .kind
        {
            TerminatorKind::Call { destinations, .. } => Some(destinations[0].local),
            _ => None,
        }
        .expect("float32 interface extraction must remain a runtime call");
    let local = wrong_destination
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.locals)
        .find(|local| local.id == destination)
        .expect("float32 interface extraction destination");
    local.ty = RustType::Float(FloatKind::F64);

    let error = wrong_destination.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("runtime call destination 0 type mismatch")
            && error.message.contains("Float(F32)")
            && error.message.contains("Float(F64)"),
        "{error:?}"
    );
}
