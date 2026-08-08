use super::compile_and_run;

#[test]
fn local_named_types_and_aliases_follow_block_scope() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                type score int
                var value score = 4
                {
                    type score string
                    var text score = "ok"
                    println(text)
                }
                type scoreAlias = score
                var copy scoreAlias = value
                println(value)
                println(copy)
            }
        "#,
    );

    assert_eq!(run.stderr, b"ok\n4\n4\n");
}

#[test]
fn declared_types_shadow_predeclared_type_names() {
    let run = compile_and_run(
        r#"
            package main

            type rune = string
            type uint16 = string

            func main() {
                converted := 6
                type int string
                var label int = "local"
                var packageAlias rune = "package"
                var unsupportedPredeclaredName uint16 = "declared"
                println(label, packageAlias, unsupportedPredeclaredName, converted)
            }
        "#,
    );

    assert_eq!(run.stderr, b"local package declared 6\n");
}
