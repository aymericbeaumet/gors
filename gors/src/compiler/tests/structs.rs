use super::compile_and_run;

#[test]
fn generated_integer_structs_support_literals_copies_and_field_reads() {
    let run = compile_and_run(
        r#"
            package main

            type Point struct {
                X int
                Y int
            }

            func sum(point Point) int {
                return point.X + point.Y
            }

            func main() {
                keyed := Point{Y: 4, X: 3}
                positional := Point{5, 6}
                duplicate := keyed
                zero := Point{}
                if sum(duplicate) != 7 || sum(positional) != 11 {
                    panic("struct literal or copy changed")
                }
                if keyed != duplicate || keyed == positional {
                    panic("struct equality changed")
                }
                if zero.X != 0 || zero.Y != 0 {
                    panic("struct zero value changed")
                }
                println("structs: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"structs: ok\n");
    assert!(run.rust.contains("[i64; 2]"), "{}", run.rust);
}

#[test]
fn generated_integer_structs_support_local_field_updates() {
    let run = compile_and_run(
        r#"
            package main

            type Counter struct { Value int }

            func (counter Counter) Added(delta int) int {
                counter.Value += delta
                return counter.Value
            }

            func main() {
                counter := Counter{Value: 3}
                counter.Value = 5
                counter.Value += 2
                counter.Value *= 3
                duplicate := counter
                duplicate.Value -= 1
                if counter.Value != 21 || duplicate.Value != 20 {
                    panic("struct field update or value copy changed")
                }
                if counter.Added(4) != 25 || counter.Value != 21 {
                    panic("value receiver field update escaped its copy")
                }
                println("struct-updates: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"struct-updates: ok\n");
    assert!(run.rust.contains("__gors_structure"), "{}", run.rust);
}

#[test]
fn generated_mixed_structs_preserve_field_types_and_value_copies() {
    let run = compile_and_run(
        r#"
            package main

            type Bag struct { Values []int }

            func (bag Bag) At(index int) int {
                return bag.Values[index]
            }

            func main() {
                bag := Bag{Values: []int{1, 2, 3}}
                duplicate := bag
                duplicate.Values = []int{7, 8}
                if bag.At(0) != 1 || bag.At(2) != 3 || duplicate.At(1) != 8 {
                    panic("mixed struct field or value copy changed")
                }
                println("mixed-structs: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"mixed-structs: ok\n");
    assert!(
        run.rust.contains("(::__gors_runtime::GoSliceI64,)"),
        "{}",
        run.rust
    );
}

#[test]
fn generated_integer_structs_support_value_methods_and_go_namespaces() {
    let run = compile_and_run(
        r#"
            package main

            type Left struct { Value int }
            type Right struct { Value int }

            func Read() int { return 1 }

            func (left Left) Read(delta int) int {
                return left.Value + delta
            }

            func (right Right) Read(delta int) int {
                return right.Value + delta + 1
            }

            func main() {
                left := Left{Value: 3}
                right := Right{Value: 4}
                if Read() != 1 || left.Read(5) != 8 || right.Read(5) != 10 {
                    panic("method identity or receiver lowering changed")
                }
                println("methods: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"methods: ok\n");
}

#[test]
fn generated_value_method_values_capture_the_receiver_copy() {
    let run = compile_and_run(
        r#"
            package main

            type Counter struct { Value int }

            func (counter Counter) Add(delta int) int {
                return counter.Value + delta
            }

            func main() {
                counter := Counter{Value: 3}
                saved := counter.Add
                counter = Counter{Value: 10}
                if saved(4) != 7 || counter.Add(4) != 14 {
                    panic("method value receiver capture changed")
                }
                println("method-values: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"method-values: ok\n");
}

#[test]
fn generated_method_expressions_adapt_value_and_pointer_receivers() {
    let run = compile_and_run(
        r#"
            package main

            type Counter struct { Value int }

            func (counter Counter) Read(delta int) int {
                return counter.Value + delta
            }

            func (counter *Counter) Add(delta int) int {
                counter.Value += delta
                return counter.Value
            }

            func main() {
                read := Counter.Read
                parenthesized := (Counter).Read
                add := (*Counter).Add
                readPointer := (*Counter).Read
                value := Counter{Value: 3}
                pointer := &Counter{Value: 10}
                if read(value, 4) != 7 || parenthesized(value, 5) != 8 {
                    panic("value method expression changed")
                }
                if add(pointer, 2) != 12 || pointer.Value != 12 {
                    panic("pointer method expression changed")
                }
                if readPointer(pointer, 6) != 18 || pointer.Value != 12 {
                    panic("pointer-to-value method expression changed")
                }
                println("method-expressions: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"method-expressions: ok\n");
}

#[test]
fn generated_pointer_method_values_capture_explicit_and_implicit_addresses() {
    let run = compile_and_run(
        r#"
            package main

            type Counter struct { Value int }

            func (counter *Counter) Add(delta int) int {
                counter.Value += delta
                return counter.Value
            }

            func main() {
                pointer := &Counter{Value: 2}
                savedPointer := pointer.Add
                pointer.Value = 4
                addressable := Counter{Value: 7}
                savedAddress := addressable.Add
                if savedPointer(3) != 7 || pointer.Value != 7 {
                    panic("explicit pointer method value changed")
                }
                if savedAddress(2) != 9 || addressable.Value != 9 {
                    panic("implicit address method value changed")
                }
                println("pointer-method-values: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"pointer-method-values: ok\n");
}

#[test]
fn promoted_methods_follow_shallowest_value_and_pointer_embedding_paths() {
    let run = compile_and_run(
        r#"
            package main

            type Value struct { n int }
            func (value Value) Read() int { return value.n }

            type Pointer struct { n int }
            func (pointer *Pointer) Add(delta int) int {
                pointer.n += delta
                return pointer.n
            }

            type Outer struct {
                Value
                *Pointer
            }

            type Counter interface {
                Add(int) int
            }

            func temporary() Outer {
                return Outer{Value: Value{n: 3}, Pointer: &Pointer{n: 4}}
            }

            func main() {
                outer := temporary()
                var counter Counter = outer
                if outer.Read() != 3 || temporary().Add(2) != 6 {
                    panic("promoted direct receiver path changed")
                }
                if counter.Add(3) != 7 || outer.Pointer.n != 7 {
                    panic("promoted interface receiver path lost pointer identity")
                }
                println("promoted-methods: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"promoted-methods: ok\n");
}

#[test]
fn promoted_value_method_dereferences_an_embedded_pointer_and_panics_when_nil() {
    let run = compile_and_run(
        r#"
            package main
            type Inner struct { value int }
            func (inner Inner) Read() int { return inner.value }
            type Outer struct { *Inner }
            func check() {
                defer func() {
                    if recover() == nil {
                        panic("nil embedded pointer did not panic")
                    }
                }()
                var outer Outer
                _ = outer.Read()
            }
            func main() {
                check()
                println("nil-promoted-receiver: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"nil-promoted-receiver: ok\n");
}

#[test]
fn equal_depth_promoted_methods_are_ambiguous() {
    let errors = crate::compiler::lower_to_hir(
        "ambiguous-method.go",
        r#"
            package main
            type Left struct{}
            func (Left) Read() int { return 1 }
            type Right struct{}
            func (Right) Read() int { return 2 }
            type Both struct { Left; Right }
            func main() { both := Both{}; println(both.Read()) }
        "#,
    )
    .unwrap_err();

    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("selector Read is ambiguous")),
        "{errors:?}"
    );
}

#[test]
fn defined_pointer_keeps_field_shorthand_but_has_no_inherited_method_set() {
    let run = compile_and_run(
        r#"
            package main
            type Base struct { value int }
            func (base *Base) Read() int { return base.value }
            type DefinedPointer *Base
            func main() {
                base := Base{value: 7}
                var pointer DefinedPointer = &base
                println(pointer.value)
            }
        "#,
    );
    assert_eq!(run.stderr, b"7\n");

    let errors = crate::compiler::lower_to_hir(
        "defined-pointer-method-set.go",
        r#"
            package main
            type Base struct { value int }
            func (base *Base) Read() int { return base.value }
            type DefinedPointer *Base
            type Reader interface { Read() int }
            func main() {
                base := Base{value: 7}
                var pointer DefinedPointer = &base
                var reader Reader = pointer
                println(reader.Read())
            }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("does not implement")),
        "{errors:?}"
    );

    let errors = crate::compiler::lower_to_hir(
        "defined-pointer-selector.go",
        r#"
            package main
            type Base struct { value int }
            func (base *Base) Read() int { return base.value }
            type DefinedPointer *Base
            func main() {
                base := Base{value: 7}
                var pointer DefinedPointer = &base
                println(pointer.Read())
            }
        "#,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("has no field or method Read")),
        "{errors:?}"
    );
}
