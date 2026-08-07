use super::compile_and_run;

#[test]
fn map_range_uses_fixed_candidates_with_live_membership_and_values() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                currentDeletes := map[string]int{"a": 1, "b": 2, "c": 3, "d": 4}
                visits, sum := 0, 0
                for key, value := range currentDeletes {
                    visits++
                    sum += value
                    delete(currentDeletes, key)
                }
                if visits != 4 || sum != 10 || len(currentDeletes) != 0 {
                    panic("deleting the current key changed candidates")
                }

                suppress := map[int]string{1: "x", 2: "y", 3: "z"}
                visits = 0
                retained := 0
                for key := range suppress {
                    visits++
                    retained = key
                    for other := range suppress {
                        if other != key { delete(suppress, other) }
                    }
                }
                if visits != 1 || len(suppress) != 1 || suppress[retained] == "" {
                    panic("deleting unreached keys did not suppress iteration")
                }

                updates := map[string]int{"a": 1, "b": 2}
                observedUpdate := false
                for key, value := range updates {
                    if value == 9 { observedUpdate = true }
                    if key == "a" { updates["b"] = 9 } else { updates["a"] = 9 }
                }
                if !observedUpdate { panic("range value was not read live") }

                println("map-delete-during-range: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"map-delete-during-range: ok\n");
    for symbol in [
        "go_map_string_i64_range_keys",
        "go_map_string_i64_contains",
        "go_map_string_i64_get",
        "go_map_i64_go_string_range_keys",
        "go_map_i64_go_string_contains",
        "go_map_i64_go_string_get",
    ] {
        assert!(run.rust.contains(symbol), "missing {symbol}:\n{}", run.rust);
    }
    assert!(
        !run.rust.contains("go_map_string_i64_key_at"),
        "{}",
        run.rust
    );
}

#[test]
fn int_string_map_supports_the_complete_concrete_operation_family() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                var nilMap map[int]string
                missing, present := nilMap[1]
                delete(nilMap, 1)
                clear(nilMap)
                if nilMap != nil || missing != "" || present || len(nilMap) != 0 {
                    panic("nil map[int]string behavior changed")
                }

                values := make(map[int]string)
                alias := values
                values[1] = "x"
                value, ok := alias[1]
                if value != "x" || !ok || len(alias) != 1 {
                    panic("map[int]string identity changed")
                }
                delete(alias, 1)
                values[2] = "y"
                clear(values)
                if len(alias) != 0 { panic("map[int]string clear changed") }
                println("map-int-string: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"map-int-string: ok\n");
    assert!(run.rust.contains("GoMapI64GoString"), "{}", run.rust);
    for suffix in [
        "nil", "make", "get", "contains", "set", "delete", "clear", "is_nil",
    ] {
        let symbol = format!("go_map_i64_go_string_{suffix}");
        assert!(
            run.rust.contains(&symbol),
            "missing {symbol}:\n{}",
            run.rust
        );
    }
}

#[test]
fn named_concrete_map_types_preserve_their_runtime_representation() {
    let run = compile_and_run(
        r#"
            package main

            type Words map[int]string
            type Counts map[string]int

            func main() {
                words := Words{1: "one", 2: "two"}
                counts := make(Counts)
                counts[words[1]] = 1
                counts[words[2]] = 2

                seen := 0
                for key, word := range words {
                    if word == "" || key == 0 { panic("named int/string range changed") }
                    seen += counts[word]
                }
                if seen != 3 { panic("named map lookup changed") }

                delete(words, 1)
                clear(counts)
                if len(words) != 1 || len(counts) != 0 {
                    panic("named map mutation changed")
                }
                println("named-maps: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"named-maps: ok\n");
}
