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
