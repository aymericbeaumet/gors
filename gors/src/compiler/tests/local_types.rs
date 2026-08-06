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
