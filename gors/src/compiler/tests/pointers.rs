use super::{compile_and_run, compile_file};

#[test]
fn generated_integer_pointers_preserve_nil_and_shared_pointee_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func write(pointer *int, value int) { *pointer = value }
            func nilReadPanics() (panicked bool) {
                defer func() { panicked = recover() != nil }()
                var pointer *int
                _ = *pointer
                return false
            }
            func main() {
                pointer := new(int)
                if pointer == nil || *pointer != 0 { panic("invalid new value") }
                alias := pointer
                write(alias, 42)
                if *pointer != 42 { panic("pointer identity changed") }
                var nilPointer *int
                if nilPointer != nil { panic("invalid nil pointer") }
                if !nilReadPanics() { panic("nil pointer read did not panic") }
                println("pointers: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"pointers: ok\n");
    assert!(run.rust.contains("GoPointerI64"), "{}", run.rust);
    assert!(run.rust.contains("go_pointer_i64_set"), "{}", run.rust);
}

#[test]
fn generated_integer_pointer_comparisons_preserve_exact_type_and_pointee_identity() {
    let run = compile_and_run(
        r#"
            package main
            type Counter int
            type IntPointer *int
            func main() {
                leftValue := 7
                rightValue := 7
                left := &leftValue
                alias := &leftValue
                right := &rightValue
                var firstNil, secondNil *int
                if left != alias || left == right || firstNil != secondNil {
                    panic("integer pointer identity changed")
                }
                *alias = 9
                if leftValue != 9 || *left != 9 {
                    panic("integer pointer alias changed")
                }
                firstNew := new(int)
                secondNew := new(int)
                if firstNew == secondNew {
                    panic("distinct allocations compared equal")
                }

                var defined IntPointer = left
                var unnamed *int = defined
                if defined != unnamed || unnamed != defined {
                    panic("defined and unnamed assignable pointers compared unequal")
                }

                namedValue := Counter(3)
                named := &namedValue
                namedAlias := named
                var namedNil *Counter
                if named != namedAlias || named == nil || namedNil != nil {
                    panic("named integer pointer comparison changed")
                }
                println("pointer-equality: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"pointer-equality: ok\n");
    assert!(run.rust.contains("go_pointer_i64_equal"), "{}", run.rust);
}

#[test]
fn integer_pointer_expression_switches_preserve_pointee_identity_and_nil() {
    let run = compile_and_run(
        r#"
            package main
            func matches(tag, candidate *int) bool {
                switch tag {
                case candidate:
                    return true
                }
                return false
            }
            func main() {
                leftValue := 7
                rightValue := 7
                left := &leftValue
                alias := &leftValue
                right := &rightValue
                var firstNil, secondNil *int
                if !matches(left, alias) || matches(left, right) || !matches(firstNil, secondNil) {
                    panic("pointer switch identity changed")
                }
                println("pointer-switch: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"pointer-switch: ok\n");
    assert!(run.rust.contains("go_pointer_i64_equal"), "{}", run.rust);
}

#[test]
fn integer_pointer_comparisons_reject_representation_compatible_distinct_types() {
    for source in [
        r#"package main
type Counter int
func main() {
    var plain *int
    var named *Counter
    _ = plain == named
}
"#,
        r#"package main
type P *int
type Q *int
func main() {
    var left P
    var right Q
    _ = left == right
}
"#,
    ] {
        let errors = compile_file("pointer-types.go", source)
            .err()
            .expect("distinct pointer types must not compare through their shared representation");

        assert!(
            errors.iter().any(|error| {
                error.code == "GORS2002"
                    && error
                        .message
                        .contains("incompatible pointer comparison operands")
            }),
            "{errors:?}"
        );
    }
}

#[test]
fn generated_integer_struct_pointers_preserve_identity_aliasing_and_value_copies() {
    let run = compile_and_run(
        r#"
            package main
            type Pair struct { Left int; Right int }
            func update(pair *Pair) {
                pair.Left += 2
                pair.Right = pair.Left + 3
            }
            func nilReadPanics() (panicked bool) {
                defer func() { panicked = recover() != nil }()
                var pair *Pair
                _ = pair.Left
                return false
            }
            func main() {
                local := Pair{Left: 1, Right: 2}
                alias := &local
                update(alias)
                if local.Left != 3 || local.Right != 6 { panic("local alias changed") }

                copied := *alias
                copied.Left = 40
                if local.Left != 3 || copied.Left != 40 { panic("struct copy changed") }

                literal := &Pair{Left: 7, Right: 8}
                same := literal
                distinct := &Pair{Left: 7, Right: 8}
                if same != literal || distinct == literal { panic("pointer identity changed") }
                literal.Right += 4
                if same.Right != 12 { panic("pointer field alias changed") }

                empty := new(Pair)
                if empty == nil || empty.Left != 0 || empty.Right != 0 {
                    panic("invalid new struct pointer")
                }
                var nilPair *Pair
                if nilPair != nil { panic("invalid nil struct pointer") }
                if !nilReadPanics() { panic("nil struct pointer read did not panic") }
                println("struct-pointers: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"struct-pointers: ok\n");
    assert!(run.rust.contains("GoPointerStructI64"), "{}", run.rust);
    assert!(
        run.rust.contains("go_pointer_struct_i64_set"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_pointer_struct_i64_equal"),
        "{}",
        run.rust
    );
}
