use super::{compile_and_run, raw_program_files};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_file};

#[test]
fn generated_scalar_arrays_preserve_literals_updates_and_evaluation_order() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                counter := 0
                next := func() int {
                    counter++
                    return counter
                }

                dynamic := [2]int{1: next(), 0: next()}
                vowels := [128]bool{'a': true, 'e': true}
                filter := [6]float64{-1, 4: -0.1, -0.1}
                days := [...]string{"Sat", "Sun"}
                saved := days

                vowels['a'] = false
                filter[1] = 0.5
                days[0] = "Fri"

                if counter != 2 || dynamic[0] != 2 || dynamic[1] != 1 {
                    panic("array element evaluation order changed")
                }
                if vowels['a'] || !vowels['e'] || vowels['i'] {
                    panic("bool array literal or update changed")
                }
                if filter[0] != -1 || filter[1] != 0.5 || filter[4] != -0.1 || filter[5] != -0.1 {
                    panic("float array literal or update changed")
                }
                if len(days) != 2 || days[0] != "Fri" || saved[0] != "Sat" || saved[1] != "Sun" {
                    panic("string array literal, copy, or update changed")
                }
                println("scalar-arrays: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"scalar-arrays: ok\n");
    assert!(run.rust.contains("[bool; 128]"), "{}", run.rust);
    assert!(run.rust.contains("[f64; 6]"), "{}", run.rust);
    assert!(run.rust.contains("GoString; 2]"), "{}", run.rust);
}

#[test]
fn integer_array_equality_preserves_the_exact_element_kind() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                bytes := [2]byte{1, 255}
                sameBytes := [2]uint8{1, 255}
                otherBytes := [2]byte{1, 0}
                println(bytes == sameBytes, bytes != otherBytes)
            }
        "#,
    );

    assert_eq!(run.stderr, b"true true\n");
}

#[test]
fn constant_array_lengths_work_in_package_function_and_local_type_positions() {
    let run = compile_and_run(
        r#"
            package main

            const text = "hé"
            const width = len(text) + 1

            type Row [width]int
            var packageZero [width]int

            func measured(value [width]int) int { return len(value) }

            func main() {
                const localWidth = width - 1
                type Local [localWidth]int
                var local Local
                literal := [width]int{width - 1: 9}
                println(len(Row{}), len(packageZero), measured(literal), len(local), literal[3])
            }
        "#,
    );

    assert_eq!(run.stderr, b"4 4 4 3 9\n");
}

#[test]
fn zero_length_arrays_and_named_array_capacity_need_no_element_representation() {
    let run = compile_and_run(
        r#"
            package main

            type Empty [len("")]struct{}
            type Unsupported [0]map[int]int
            type Values [3]int

            var packageEmpty Empty
            var packageUnsupported Unsupported

            func main() {
                var localEmpty [0]struct{}
                var localUnsupported [0]map[int]int
                var values Values
                println(len(packageEmpty), cap(packageEmpty), len(localEmpty), cap(localEmpty))
                println(len(packageUnsupported), cap(packageUnsupported), len(localUnsupported), cap(localUnsupported))
                println(len(values), cap(values))
            }
        "#,
    );

    assert_eq!(run.stderr, b"0 0 0 0\n0 0 0 0\n3 3\n");
    assert!(run.rust.contains("[(); 0]"), "{}", run.rust);
}

#[test]
fn invalid_constant_array_lengths_are_rejected_precisely() {
    for (source, expected) in [
        (
            "package main\nfunc main() { size := 2; var value [size]int; println(len(value)) }\n",
            "size is not a constant",
        ),
        (
            "package main\nconst size = -1\nfunc main() { var value [size]int; println(len(value)) }\n",
            "array length must be non-negative",
        ),
        (
            "package main\nconst size = 1.5\nfunc main() { var value [size]int; println(len(value)) }\n",
            "array length must be an integer constant",
        ),
        (
            "package main\nconst size = 9223372036854775808\nfunc main() { var value [size]int; println(len(value)) }\n",
            "array length is not representable by Go int",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("invalid array length must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.code == "GORS2002" && error.message.contains(expected)),
            "expected {expected:?}, got {errors:?}"
        );
    }
}

#[test]
fn constant_length_edits_rebuild_only_dependent_semantic_roots() {
    let before = raw_program_files([
        (
            "constants.go",
            "constants.go",
            "package main\nconst size = 2\n",
        ),
        (
            "main.go",
            "main.go",
            "package main\nfunc main() { var values [size]int; println(len(values), stable()) }\n",
        ),
        (
            "stable.go",
            "stable.go",
            "package main\nfunc stable() int { return 7 }\n",
        ),
    ]);
    let after = raw_program_files([
        (
            "constants.go",
            "constants.go",
            "package main\nconst size = 3\n",
        ),
        (
            "main.go",
            "main.go",
            "package main\nfunc main() { var values [size]int; println(len(values), stable()) }\n",
        ),
        (
            "stable.go",
            "stable.go",
            "package main\nfunc stable() int { return 7 }\n",
        ),
    ]);
    let mut session = CompilerSession::default();
    session.compile_program(before).unwrap();
    session.database().reset_telemetry();

    session.compile_program(after.clone()).unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedConstant), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);

    session.database().reset_telemetry();
    session.compile_program(after).unwrap();
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn sparse_inferred_arrays_and_slices_use_the_largest_constant_index() {
    let run = compile_and_run(
        r#"
            package main

            type index int

            func main() {
                a := [...]int{9: 1}
                b := [...]int{2: 20, 30, 0: 10}
                const key = 5
                c := [...]string{key: "five", "six"}
                d := [...]int{index(3): 42}
                s := []int{9: 90, 100}
                words := []string{1: "one", 3: "three"}

                println(len(a), a[0], a[9])
                println(len(b), b[0], b[1], b[2], b[3])
                println(len(c), c[0] == "", c[5], c[6])
                println(len(d), d[3])
                println(len(s), cap(s), s[9], s[10], s[4])
                println(len(words), words[0] == "", words[1], words[2] == "", words[3])
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        b"10 0 1\n4 10 0 20 30\n7 true five six\n4 42\n11 11 90 100 0\n4 true one true three\n"
    );
}
