use super::compile_and_run;

#[test]
fn integer_pointer_interfaces_preserve_nil_identity_aliasing_and_assertions() {
    let run = compile_and_run(
        r#"
            package main

            type Counter int
            type Alias = int

            func assertWrongTypePanics(value any) {
                defer func() {
                    if recover() == nil {
                        panic("wrong pointer assertion did not panic")
                    }
                    println("pointer-interface: ok")
                }()
                _ = value.(*Counter)
            }

            func main() {
                var nilPointer *int
                var boxedNil any = nilPointer
                if boxedNil == nil {
                    panic("typed nil pointer became a nil interface")
                }
                nilAlias, ok := boxedNil.(*int)
                if !ok || nilAlias != nil {
                    panic("typed nil pointer assertion changed the value")
                }

                var aliasPointer *Alias
                var boxedAlias any = aliasPointer
                if value, ok := boxedAlias.(*int); !ok || value != nil {
                    panic("alias pointer did not retain builtin identity")
                }

                value := 7
                pointer := &value
                var first any = pointer
                var same any = pointer
                otherValue := 7
                var different any = &otherValue
                if first != same || first == different {
                    panic("interface pointer equality lost pointee identity")
                }
                extracted := first.(*int)
                *extracted = 41
                if value != 41 {
                    panic("interface pointer extraction lost aliasing")
                }

                namedValue := Counter(3)
                namedPointer := &namedValue
                var named any = namedPointer
                if _, ok := named.(*Counter); !ok {
                    panic("named integer pointer lost dynamic identity")
                }
                if _, ok := named.(*int); ok {
                    panic("named integer pointer matched builtin pointer")
                }

                assertWrongTypePanics(first)
            }
        "#,
    );

    assert_eq!(run.stderr, b"pointer-interface: ok\n");
    assert!(
        run.rust.contains("go_interface_box_pointer_i64"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_interface_unbox_pointer_i64"),
        "{}",
        run.rust
    );
}
