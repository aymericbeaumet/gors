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

#[test]
fn string_rune_slice_conversions_preserve_go_decoding_types_and_copying() {
    let run = compile_and_run(
        r#"
            package main

            type Text string
            type Rune rune
            type Runes []Rune

            func main() {
                source := Text("h\xff\u00e9")
                first := Runes(source)
                second := Runes(source)
                first[0] = 'X'
                if source != "h\xff\u00e9" || second[0] != 'h' {
                    panic("string to rune conversion reused mutable backing")
                }
                if len(first) != 3 || first[1] != '\uFFFD' || first[2] != 0xE9 {
                    panic("string to rune conversion decoded invalid UTF-8 incorrectly")
                }
                empty := Runes("")
                if len(empty) != 0 {
                    panic("empty string conversion did not produce an empty slice")
                }
                rebuilt := Text(Runes{0x266B, 0x1F30D})
                println(string(first), string(second), rebuilt, len(source), len(string(second)))
            }
        "#,
    );

    assert_eq!(run.stderr, "X�é h�é ♫🌍 4 6\n".as_bytes());
    assert!(
        run.rust.contains("go_string_to_slice_runes"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_string_from_slice_runes"),
        "{}",
        run.rust
    );
}
