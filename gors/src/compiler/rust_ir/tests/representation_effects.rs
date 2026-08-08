use super::*;

#[test]
fn representation_effects_cover_runtime_calls_clones_and_string_allocation() {
    let file = lower(
        r#"
            package main
            func join(left string, right string) string { return left + right }
            func identity(value string) string { saved := value; return saved }
            func add(left int, right int) int { return left + right }
            func equal(left string, right string) bool { return left == right }
            func notEqual(left string, right string) bool { return left != right }
            func less(left string, right string) bool { return left < right }
            func lessEqual(left string, right string) bool { return left <= right }
            func greater(left string, right string) bool { return left > right }
            func greaterEqual(left string, right string) bool { return left >= right }
            func main() {
                value := "x"
                saved := value
                println(join(value, "y"), add(1, 2))
                println(saved)
            }
        "#,
    );

    let concat = binary_rvalue(&file, ValueOp::Runtime(RuntimeOp::ConcatGoStrings));
    assert!(concat.effects.may_read);
    assert!(concat.effects.may_call);
    assert!(concat.effects.may_allocate);
    assert!(concat.effects.may_write);
    assert!(!concat.effects.may_panic);
    assert_eq!(concat.panic, PanicEdge::None);

    let add = binary_rvalue(
        &file,
        ValueOp::Primitive(PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingAdd,
            kind: IntegerKind::I64,
        }),
    );
    assert!(add.effects.may_read);
    assert!(!add.effects.may_call);
    assert!(!add.effects.may_allocate);
    assert!(!add.effects.may_panic);

    for operation in [
        PrimitiveOp::StringEqual,
        PrimitiveOp::StringNotEqual,
        PrimitiveOp::StringLess,
        PrimitiveOp::StringLessEqual,
        PrimitiveOp::StringGreater,
        PrimitiveOp::StringGreaterEqual,
    ] {
        let comparison = binary_rvalue(&file, ValueOp::Primitive(operation));
        assert!(comparison.effects.may_read);
        assert!(comparison.effects.may_call);
        assert!(!comparison.effects.may_allocate);
        assert!(!comparison.effects.may_panic);
    }

    let string_literal = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Constant(Constant::RuntimeStaticBytes {
                    op: RuntimeOp::GoStringFromStatic,
                    ..
                }))
            )
        })
        .unwrap();
    assert!(string_literal.effects.may_call);
    assert!(!string_literal.effects.may_allocate);

    let clone_read = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Read {
                    op: ReadOp::ProvenInitializedClone,
                    ..
                })
            )
        })
        .unwrap();
    assert!(clone_read.effects.may_read);
    assert!(clone_read.effects.may_call);
    assert!(!clone_read.effects.may_allocate);
    assert!(!clone_read.effects.may_panic);

    let move_read = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Read {
                    op: ReadOp::ProvenLastUseMove,
                    ..
                })
            )
        })
        .unwrap();
    assert!(move_read.effects.may_read);
    assert!(move_read.effects.may_write);
    assert!(!move_read.effects.may_call);
    assert!(!move_read.effects.may_allocate);
    assert!(!move_read.effects.may_panic);
    assert_eq!(move_read.panic, PanicEdge::None);
}
