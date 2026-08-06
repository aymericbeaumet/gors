use super::{compile_and_run, raw_program};
use crate::compiler::compile_program;

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
