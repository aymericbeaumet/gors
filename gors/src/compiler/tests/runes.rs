use super::compile_and_run;

#[test]
fn rune_escapes_and_integer_to_string_conversion_execute_with_go_semantics() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                println('\xff')
                println('\u12e4')
                println('\U00101234')
                println('ä')
                println(string('\377'))
                println(len(string('\377')))
                invalid := -1
                println(string(invalid))
            }
        "#,
    );

    assert_eq!(run.stderr, "255\n4836\n1053236\n228\nÿ\n2\n�\n".as_bytes());
    assert!(run.rust.contains("go_string_from_rune"), "{}", run.rust);
}
