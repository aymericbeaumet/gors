use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_program};

#[test]
fn float_interfaces_preserve_ieee_equality_and_extract_float64() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                huge := 1e308
                infinity := huge + huge
                nan := infinity + -infinity
                zero := 0.0
                negativeZero := -zero

                var nanLeft any = nan
                var nanRight any = nan
                var zeroLeft any = zero
                var zeroRight any = negativeZero
                println(nan == nan, negativeZero == zero)
                println(nanLeft == nanRight, zeroLeft == zeroRight)

                var value any = 3.5
                extracted, ok := value.(float64)
                println(ok, extracted)
            }
        "#,
    );

    assert_eq!(run.stderr, b"false true\nfalse true\ntrue 3.5\n");
    assert!(run.rust.contains("go_interface_box_f64"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_equal"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_unbox_f64"), "{}", run.rust);
    assert!(run.rust.contains("print_f64"), "{}", run.rust);
}

#[test]
fn float32_conversions_round_before_interface_boxing() {
    let run = compile_and_run(
        r#"
            package main
            type Narrow float32
            func main() {
                wide := 16777217.0
                narrow := float32(wide)
                if float64(narrow) != 16777216.0 {
                    panic("float32 conversion did not round")
                }
                var boxed any = narrow
                extracted, ok := boxed.(float32)
                var named any = Narrow(narrow)
                namedExtracted, namedOK := named.(Narrow)
                println(ok, float64(extracted), namedOK, float64(namedExtracted))
            }
        "#,
    );

    assert_eq!(run.stderr, b"true 1.6777216e+07 true 1.6777216e+07\n");
    assert!(run.rust.contains("as f32"), "{}", run.rust);
    assert!(run.rust.contains("builtin:float32"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_box_f32"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_unbox_f32"), "{}", run.rust);
}

#[test]
fn float_interface_comment_edits_reuse_semantic_stages() {
    let before = r#"
        package main
        func compare(value float64) bool {
            var boxed any = value
            return boxed == any(value)
        }
        func main() { println(compare(1.5)) }
    "#;
    let after = r#"
        package main
        func compare(value float64) bool {
            // Float boxing and interface equality are unchanged.
            var boxed any = value
            return boxed == any(value)
        }
        func main() { println(compare(1.5)) }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    session.database().reset_telemetry();

    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
}

#[test]
fn interfaces_remain_invalid_ordered_comparison_operands() {
    let error = compile_program(raw_program(
        "interfaces.go",
        "interfaces.go",
        "package main\nfunc main() { var left any = 1.0; var right any = 2.0; _ = left < right }\n",
    ))
    .err()
    .expect("ordered interface comparison must fail");

    assert!(
        error
            .to_string()
            .contains("operator Less is invalid for Interface"),
        "{error}"
    );
}

#[test]
fn generated_interfaces_preserve_nil_dynamic_types_and_value_copies() {
    let run = compile_and_run(
        r#"
            package main

            type Reader interface { Read() int }
            type Counter struct { Value int }
            func (counter Counter) Read() int { return counter.Value }

            func present(value any) bool { return value != nil }

            func main() {
                var zero any
                println(zero == nil)

                var integer any = 42
                var text any = "gors"
                println(integer != nil)
                println(text != nil)

                original := Counter{Value: 7}
                var reader Reader = original
                original.Value = 9
                copy := reader
                println(reader != nil)
                println(copy != nil)
                println(present("compiler"))
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
    assert!(run.rust.contains("GoInterface"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_nil"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_box_i64"), "{}", run.rust);
    assert!(
        run.rust.contains("go_interface_box_struct_i64"),
        "{}",
        run.rust
    );
}

#[test]
fn any_conversions_box_values_like_interface_assignment() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                x := 42
                if _, ok := any(x).(int); !ok {
                    panic("any conversion lost the dynamic type")
                }
                boxed := any("text")
                if value, ok := boxed.(string); !ok || value != "text" {
                    panic("any conversion changed the value")
                }
                var v any = any(true)
                println(v != nil)
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\n");
    assert!(run.rust.contains("go_interface_box_i64"), "{}", run.rust);
}

#[test]
fn interface_conversions_require_implementing_operands() {
    let error = compile_program(raw_program(
        "interfaces.go",
        "interfaces.go",
        r#"
            package main
            type Reader interface { Read() int }
            func main() { _ = Reader(1) }
        "#,
    ))
    .err()
    .expect("converting a non-implementing operand to an interface must fail");

    assert!(error.to_string().contains("does not implement"), "{error}");
}

#[test]
fn interface_assignment_checks_the_complete_method_set() {
    let error = compile_program(raw_program(
        "interfaces.go",
        "interfaces.go",
        r#"
            package main
            type Reader interface { Read() int }
            func main() { var reader Reader = 1; _ = reader }
        "#,
    ))
    .err()
    .expect("a concrete value without Read must not implement Reader");

    assert!(error.to_string().contains("does not implement"), "{error}");
}

#[test]
fn interface_method_calls_dispatch_to_value_and_pointer_receivers() {
    let run = compile_and_run(
        r#"
            package main

            type Reader interface { Read() int }
            type Adder interface { Add(int) int }
            type Counter struct { Value int }

            func (counter Counter) Read() int { return counter.Value }
            func (counter *Counter) Add(delta int) int {
                counter.Value += delta
                return counter.Value
            }
            func read(reader Reader) int { return reader.Read() }

            func main() {
                var value Reader = Counter{Value: 7}
                println(read(value))

                pointer := &Counter{Value: 9}
                var pointerReader Reader = pointer
                println(pointerReader.Read())

                var adder Adder = pointer
                println(adder.Add(-2))
                println(pointer.Value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"7\n9\n7\n7\n");
    assert!(run.rust.contains("go_interface_is_type"), "{}", run.rust);
    assert!(
        run.rust.contains("go_interface_unbox_pointer_struct_i64"),
        "{}",
        run.rust
    );
}

#[test]
fn multi_result_assignments_box_interface_destinations_before_writes() {
    let run = compile_and_run(
        r#"
            package main

            func pair() (int, int) { return 7, 8 }

            func main() {
                var boxed any
                number := 0
                boxed, number = pair()
                println(boxed != nil)
                println(number)
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\n8\n");
    assert!(run.rust.contains("go_interface_box_i64"), "{}", run.rust);
}

#[test]
fn type_switches_bind_concrete_and_interface_case_values() {
    let run = compile_and_run(
        r#"
            package main

            type Marker struct { Value int }

            func classify(value any) int {
                switch dynamic := value.(type) {
                case int:
                    return dynamic + 1
                case nil:
                    if dynamic == nil { return 0 }
                    return -10
                case bool, string:
                    if dynamic != nil { return 2 }
                    return -20
                default:
                    if dynamic != nil { return 3 }
                    return -30
                }
            }

            func main() {
                var empty any
                println(classify(4), classify(empty), classify(true), classify(Marker{Value: 1}))
            }
        "#,
    );

    assert_eq!(run.stderr, b"5 0 2 3\n");
    assert!(run.rust.contains("go_interface_is_type"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_unbox_i64"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_is_nil"), "{}", run.rust);
}

#[test]
fn nil_interface_method_calls_panic_instead_of_running_a_zero_receiver() {
    // Dispatch tests every candidate, so a receiver carrying no dynamic type
    // reaches none of them. Without an explicit failure the nil call would fall
    // through to whichever candidate happened to be emitted last, and a
    // zero-field receiver extracts without ever inspecting the boxed value, so
    // the call would silently run on a zero receiver instead of panicking.
    let run = compile_and_run(
        r#"
            package main

            type talker interface{ Talk() string }

            type speaker struct{}

            func (speaker) Talk() string { return "speaker" }

            type shouter struct{ volume int }

            func (s shouter) Talk() string { return "shouter" }

            func callNil() (recovered interface{}) {
                defer func() { recovered = recover() }()
                var value talker
                _ = value.Talk()
                return nil
            }

            func main() {
                var value talker = speaker{}
                println(value.Talk())
                value = shouter{volume: 1}
                println(value.Talk())

                recovered := callNil()
                println(recovered == nil)
                _, isError := recovered.(error)
                println(isError)
            }
        "#,
    );

    assert_eq!(run.stderr, b"speaker\nshouter\nfalse\ntrue\n");
}

#[test]
fn integer_slices_box_and_extract_through_interfaces() {
    // An integer slice carries its own dynamic identity, so it can be a field
    // of an aggregate struct and can round-trip through an interface. The
    // identity is per element kind, so an assertion to a different slice type
    // must fail rather than alias the shared carrier.
    let run = compile_and_run(
        r#"
            package main

            type Value struct {
                Items []int
                Count int
            }

            func main() {
                value := Value{Items: []int{1, 2, 3}, Count: 7}
                p := &value
                println(len(p.Items), p.Items[1], p.Count)

                var boxed interface{} = value.Items
                got, ok := boxed.([]int)
                println(ok, len(got), got[2])

                _, wrongElement := boxed.([]string)
                _, wrongWidth := boxed.([]int32)
                println(wrongElement, wrongWidth)

                var pointers interface{} = []uintptr{9}
                back, isPointers := pointers.([]uintptr)
                println(isPointers, back[0])
            }
        "#,
    );

    assert_eq!(run.stderr, b"3 2 7\ntrue 3 3\nfalse false\ntrue 9\n");
}
