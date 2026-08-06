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
