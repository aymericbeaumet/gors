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
fn len_cap_constants_suppress_array_operands_but_runtime_arrays_evaluate_once() {
    let source = r#"
        package main

        func makeArray(evaluations *int) [4]int {
            *evaluations = *evaluations + 1
            return [4]int{1, 2, 3, 4}
        }

        func main() {
            const pointerLen = len((*[7]int)(nil))
            const pointerCap = cap((*[7]int)(nil))
            const literalLen = len([10]float64{imag(2i)})
            if pointerLen != 7 || pointerCap != 7 || literalLen != 10 {
                panic("constant array len/cap changed")
            }

            evaluations := 0
            gotLen := len(makeArray(&evaluations))
            gotCap := cap(makeArray(&evaluations))
            if gotLen != 4 || gotCap != 4 || evaluations != 2 {
                panic("runtime array operand evaluation changed")
            }

            var nilSlice []int
            var nilMap map[int]string
            var nilChannel chan int
            if len(nilSlice) != 0 || cap(nilSlice) != 0 || len(nilMap) != 0 ||
                len(nilChannel) != 0 || cap(nilChannel) != 0 {
                panic("nil len/cap changed")
            }
            const stringBytes = len("héllo")
            if stringBytes != 6 { panic("string len is not a byte count") }
            println("len-cap-constant-and-nil: ok")
        }
    "#;
    let hir = crate::compiler::lower_to_hir("len-cap.go", source).unwrap();
    let main = hir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let runtime_lengths = main
        .body
        .stmts
        .iter()
        .filter_map(|statement| match &statement.kind {
            crate::compiler::hir::StmtKind::Let { values, .. } => values.first(),
            _ => None,
        })
        .filter_map(|expression| match &expression.kind {
            crate::compiler::hir::ExprKind::ArrayLen { array, length } => {
                Some((array.as_ref(), *length, expression.effects))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(runtime_lengths.len(), 2, "{main:#?}");
    for (operand, length, effects) in runtime_lengths {
        assert_eq!(length, 4);
        assert!(matches!(
            operand.kind,
            crate::compiler::hir::ExprKind::Call { .. }
        ));
        assert!(effects.may_call);
    }

    let run = compile_and_run(source);
    assert_eq!(run.stderr, b"len-cap-constant-and-nil: ok\n");
}

#[test]
fn package_constants_resolve_named_pointer_to_array_operands() {
    let run = compile_and_run(
        r#"
            package main
            type Seven [7]int
            const namedLength = len((*Seven)(nil))
            const namedCapacity = cap((*Seven)(nil))
            var values [namedLength]int
            func main() { println(namedLength, namedCapacity, len(values)) }
        "#,
    );

    assert_eq!(run.stderr, b"7 7 7\n");
}

#[test]
fn package_constant_len_reads_an_inferred_array_variable_type() {
    let run = compile_and_run(
        r#"
            package main
            var values = [3]int{0: 1, 2: 3}
            const width = len(values)
            type Copy [width]int
            func main() {
                println(width, len(values), len(Copy{}), values[0], values[1], values[2])
            }
        "#,
    );

    assert_eq!(run.stderr, b"3 3 3 1 0 3\n");
}

#[test]
fn package_array_variable_length_cycles_are_structured_failures() {
    let errors = compile_file(
        "main.go",
        r#"
            package main
            var values [width]int
            const width = len(values)
            type Copy [width]int
            func main() { println(width) }
        "#,
    )
    .err()
    .expect("package variable/constant cycle must be rejected without a query panic");

    assert!(
        errors.iter().any(|error| error
            .message
            .contains("package initialization cycle: width -> values -> width")),
        "{errors:?}"
    );
}

#[test]
fn shadowed_predeclared_type_calls_remain_runtime_array_operands() {
    let run = compile_and_run(
        r#"
            package main
            func uint8(value byte) byte {
                println("called")
                return value
            }
            func main() { println(len([1]byte{uint8(1)})) }
        "#,
    );

    assert_eq!(run.stderr, b"called\n1\n");
}

#[test]
fn incomplete_named_arrays_expose_zero_length_during_type_resolution() {
    let run = compile_and_run(
        r#"
            package main

            const zero = len((*Zero)(nil))
            type Zero [zero]int

            const one = len((*One)(nil)) + 1
            type One [one]int

            type Direct [len((*Direct)(nil))]int
            type FromComplete [len((*Three)(nil))]int
            type Three [3]int

            type Alias = ThroughAlias
            type ThroughAlias [len((*Alias)(nil))]int

            type Defined ThroughDefined
            type ThroughDefined [len((*Defined)(nil))]int

            type EarlyAlias = LateAlias
            type LateAlias [len((*EarlyAlias)(nil)) + 1]int

            type EarlyDefined LateDefined
            type LateDefined [len((*EarlyDefined)(nil)) + 1]int

            type Forward [len((*ForwardAlias)(nil)) + 1]int
            type ForwardAlias = Forward

            type ForwardNamed [len((*ForwardDefined)(nil)) + 1]int
            type ForwardDefined ForwardNamed

            func main() {
                println(zero, len(Zero{}), one, len(One{}), len(Direct{}))
                println(len(FromComplete{}), len(ThroughAlias{}), len(ThroughDefined{}))
                println(len(EarlyAlias{}), len(EarlyDefined{}))
                println(len(Forward{}), len(ForwardAlias{}))
                println(len(ForwardNamed{}), len(ForwardDefined{}))
            }
        "#,
    );

    assert_eq!(run.stderr, b"0 0 1 1 0\n3 0 0\n1 1\n1 1\n1 1\n");
}

#[test]
fn array_length_constant_cycles_are_structured_semantic_failures() {
    let errors = compile_file(
        "main.go",
        r#"
            package main
            const first = second
            const second = first
            type Values [first]int
            func main() { println(len(Values{})) }
        "#,
    )
    .err()
    .expect("constant cycle must be rejected without a query panic");

    assert!(
        errors.iter().any(|error| error
            .message
            .contains("constant initialization cycle: first -> second -> first")),
        "{errors:?}"
    );
}

#[test]
fn check_only_len_cap_operands_still_reject_invalid_semantics() {
    for (source, expected) in [
        (
            "package main\nfunc main() { const n = len([1]bool{true + false}); println(n) }\n",
            "operator Add is invalid",
        ),
        (
            "package main\nfunc main() { const n = len([1]*int{&1}); println(n) }\n",
            "cannot take the address",
        ),
        (
            "package main\nfunc next() int { return 1 }\nfunc main() { const n = len([1]int{next()}); println(n) }\n",
            "len expression is not constant",
        ),
        (
            "package main\nfunc main() { var input chan int; const n = len([1]int{<-input}); println(n) }\n",
            "len expression is not constant",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("invalid suppressed operand must be rejected");
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "expected {expected:?}, got {errors:?}"
        );
    }
}

#[test]
fn user_definitions_named_len_and_cap_do_not_dispatch_to_builtins() {
    let run = compile_and_run(
        r#"
            package main
            func len(value int) int { return value + 90 }
            func cap(value int) int { return value + 70 }
            func main() { println(len(9), cap(7)) }
        "#,
    );
    assert_eq!(run.stderr, b"99 77\n");

    let errors = compile_file(
        "main.go",
        "package main\nfunc len(value string) int { return 99 }\nconst n = len(\"a\")\nfunc main() { println(n) }\n",
    )
    .err()
    .expect("a shadowed len call is not a constant builtin expression");
    assert!(
        errors.iter().any(|error| error
            .message
            .contains("len does not resolve to the predeclared builtin")),
        "{errors:?}"
    );
}

#[test]
fn nonconstant_builtin_calls_make_outer_array_len_runtime() {
    let source = r#"
        package main
        func main() {
            values := []int{1, 2}
            measured := len([1]int{len(values)})
            println(measured)
        }
    "#;
    let hir = crate::compiler::lower_to_hir("nested-len.go", source).unwrap();
    let main = hir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    assert!(main.body.stmts.iter().any(|statement| {
        matches!(
            &statement.kind,
            crate::compiler::hir::StmtKind::Let { values, .. }
                if values.iter().any(|value| matches!(value.kind, crate::compiler::hir::ExprKind::ArrayLen { .. }))
        )
    }), "{main:#?}");

    let run = compile_and_run(source);
    assert_eq!(run.stderr, b"1\n");
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
fn suppressed_array_operand_edits_stay_green_until_the_constant_changes() {
    let program = |literal: &'static str| {
        raw_program_files([
            ("constant.go", "constant.go", literal),
            (
                "main.go",
                "main.go",
                "package main\nfunc main() { println(width) }\n",
            ),
        ])
    };
    let before = program("package main\nconst width = len([4]int{1})\n");
    let same_value = program("package main\nconst width = len([4]int{2})\n");
    let changed_value = program("package main\nconst width = len([5]int{2})\n");
    let mut session = CompilerSession::default();
    session.compile_program(before).unwrap();
    session.database().reset_telemetry();

    session.compile_program(same_value).unwrap();
    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedConstant), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);

    session.database().reset_telemetry();
    session.compile_program(changed_value).unwrap();
    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::TypedConstant), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
}

#[test]
fn unrelated_type_edits_do_not_recompute_recursive_length_constants() {
    let before = raw_program_files([
        (
            "cycle.go",
            "cycle.go",
            "package main\nconst one = len((*One)(nil)) + 1\ntype One [one]int\n",
        ),
        (
            "main.go",
            "main.go",
            "package main\nfunc main() { println(one, len(One{})) }\n",
        ),
        ("spare.go", "spare.go", "package main\ntype Spare [2]int\n"),
    ]);
    let after = raw_program_files([
        (
            "cycle.go",
            "cycle.go",
            "package main\nconst one = len((*One)(nil)) + 1\ntype One [one]int\n",
        ),
        (
            "main.go",
            "main.go",
            "package main\nfunc main() { println(one, len(One{})) }\n",
        ),
        ("spare.go", "spare.go", "package main\ntype Spare [3]int\n"),
    ]);
    let mut session = CompilerSession::default();
    session.compile_program(before).unwrap();
    session.database().reset_telemetry();

    session.compile_program(after).unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::PackageTypeLookup), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedConstant), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
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

#[test]
fn package_array_initializers_infer_ellipsis_lengths_from_their_elements() {
    // A `[...]T` literal takes its length from the literal itself: one past the
    // highest initialized index, where an unkeyed element continues from the
    // previous one. Package-level initializers must count that length rather
    // than evaluate the elided length as a constant expression.
    let run = compile_and_run(
        r#"
            package main

            type Experiment uint

            const NoExperiment Experiment = 0

            const (
                AllocFree Experiment = 1 + iota
                NumExperiments
            )

            var experiments = [...]string{
                NoExperiment: "None",
                AllocFree:    "AllocFree",
            }

            var plain = [...]int{10, 20, 30}
            var sparse = [...]int{5: 50, 2: 20}
            var resumed = [...]int{2: 20, 30, 40}
            var empty = [...]int{}

            func main() {
                println(len(experiments), experiments[0], experiments[1])
                println(len(plain), plain[2])
                println(len(sparse), sparse[5], sparse[2], sparse[0])
                println(len(resumed), resumed[3], resumed[4])
                println(len(empty))
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        b"2 None AllocFree\n3 30\n6 50 20 0\n5 30 40\n0\n"
    );
}
