use super::{compile_and_run, raw_program};
use crate::compiler::{CompilerSession, compile_program, fingerprint, hir};

mod method_constraints;

#[test]
fn generic_inference_lets_typed_arguments_determine_type_parameters() {
    let run = compile_and_run(
        r#"
            package main

            func pickSame[T any](a, b T) T { return a }

            func main() {
                var v float64 = 7
                if pickSame(1, v) != 1.0 {
                    panic("a typed argument must determine T")
                }
                if pickSame(v, 2)+0.5 != 7.5 {
                    panic("an untyped constant must adapt to the typed argument")
                }
                if pickSame(1, 2.5)+0.5 != 1.5 {
                    panic("untyped constants must merge default types")
                }
                if pickSame(2, 3) != 2 {
                    panic("matching untyped constants changed")
                }
                println("generic-inference: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"generic-inference: ok\n");
}

#[test]
fn generic_inference_rejects_unmergeable_untyped_constant_kinds() {
    let error = compile_program(raw_program(
        "generics.go",
        "generics.go",
        "package main\nfunc pickSame[T any](a, b T) T { return a }\nfunc main() { _ = pickSame(\"x\", 1) }\n",
    ))
    .err();
    assert!(
        error.is_some(),
        "mismatched untyped constant kinds must be rejected"
    );
    let Some(error) = error else {
        return;
    };
    assert!(
        error.to_string().contains("mismatched default types"),
        "{error}"
    );
}

#[test]
fn generic_inference_rejects_non_representable_untyped_constants() {
    let error = compile_program(raw_program(
        "generics.go",
        "generics.go",
        "package main\nfunc pickSame[T any](a, b T) T { return a }\nfunc main() { var v float64 = 7; _ = pickSame(v, \"x\") }\n",
    ))
    .err();
    assert!(
        error.is_some(),
        "an unassignable constant argument must be rejected"
    );
    let Some(error) = error else {
        return;
    };
    assert!(error.to_string().contains("cannot use"), "{error}");
}

#[test]
fn generic_types_accept_multiple_explicit_type_arguments() {
    let run = compile_and_run(
        r#"
            package main

            type Pair[A, B any] struct {
                First A
                Second B
            }

            func main() {
                pair := Pair[int, string]{First: 42, Second: "answer"}
                if pair.First != 42 || pair.Second != "answer" {
                    panic("multiple generic type arguments changed")
                }
                println(pair.First, pair.Second)
            }
        "#,
    );

    assert_eq!(run.stderr, b"42 answer\n");
}

#[test]
fn blank_type_parameter_names_may_repeat_and_remain_positional() {
    let run = compile_and_run(
        r#"
            package main

            func drop[_ any](value int) int { return value + 1 }
            func blanks[_, _ any]() string { return "ok" }

            func main() {
                println(drop[string](3), blanks[int, string]())
            }
        "#,
    );

    assert_eq!(run.stderr, b"4 ok\n");
}

#[test]
fn generic_calls_combine_explicit_arguments_constraints_and_call_inference() {
    let run = compile_and_run(
        r#"
            package main

            func second[A, B any](a A, b B) B { return b }
            func head[S ~[]E, E any](values S) E { return values[0] }
            func fortyTwo[P int]() P { return 42 }
            func pair[A string, B any](b B) (A, B) { return "a", b }

            func main() {
                println(second[int, string](1, "two"))
                println(head[[]int]([]int{7, 8}))
                println(fortyTwo())
                first, second := pair(9)
                println(first, second)
            }
        "#,
    );

    assert_eq!(run.stderr, b"two\n7\n42\na 9\n");
}

#[test]
fn generic_calls_lower_side_effectful_closure_arguments_once() {
    let source = r#"
        package main

        func identity[T any](value T) T { return value }

        func main() {
            side := func() int {
                println("side")
                return 7
            }
            println(identity(side()))
        }
    "#;
    let program = || raw_program("generics.go", "generics.go", source);
    let mut first_session = CompilerSession::default();
    let first_compile = first_session.compile_program(program());
    assert!(first_compile.is_ok(), "compile generic program");
    let Ok(_) = first_compile else {
        return;
    };
    let first_file = first_session.database().active_files().first().copied();
    assert!(
        first_file.is_some(),
        "compiled program must retain its source file"
    );
    let Some(first_file) = first_file else {
        return;
    };
    let first_analysis = first_session.database().analyze_file(first_file);
    assert!(first_analysis.is_ok(), "analyze generic source");
    let Ok(first_analysis) = first_analysis else {
        return;
    };
    let main_id = first_analysis
        .functions()
        .iter()
        .find(|function| function.name() == "main")
        .map(|function| function.id());
    assert!(main_id.is_some(), "main function");
    let Some(main_id) = main_id else {
        return;
    };
    let main = first_session.database().typed_hir(first_file, main_id);
    assert!(main.is_ok(), "typed generic HIR");
    let Ok(main) = main else {
        return;
    };
    let main = main.function();
    assert_eq!(
        main.closures.len(),
        2,
        "one source closure and one instantiated generic closure"
    );
    let print_arguments = main
        .body
        .stmts
        .iter()
        .find_map(|statement| match &statement.kind {
            hir::StmtKind::Expr(hir::Expr {
                kind:
                    hir::ExprKind::Call {
                        callee: hir::Callee::Builtin(hir::Builtin::Println),
                        args,
                    },
                ..
            }) => Some(args),
            _ => None,
        });
    assert!(print_arguments.is_some(), "println statement");
    let Some(print_arguments) = print_arguments else {
        return;
    };
    assert_eq!(
        print_arguments.len(),
        1,
        "println must receive exactly the generic result"
    );
    let generic_call = print_arguments.first();
    assert!(
        generic_call.is_some(),
        "println must receive the generic result"
    );
    let Some(generic_call) = generic_call else {
        return;
    };
    let generic_arguments = match &generic_call.kind {
        hir::ExprKind::Call {
            callee: hir::Callee::Closure(_),
            args,
        } => Some(args),
        _ => None,
    };
    assert!(
        generic_arguments.is_some(),
        "identity must lower to its instantiated closure"
    );
    let Some(generic_arguments) = generic_arguments else {
        return;
    };
    assert_eq!(
        generic_arguments.len(),
        1,
        "identity must receive exactly one argument"
    );
    let closure_call = generic_arguments.first();
    assert!(closure_call.is_some(), "identity must receive one argument");
    let Some(closure_call) = closure_call else {
        return;
    };
    assert!(
        matches!(
            &closure_call.kind,
            hir::ExprKind::Call {
                callee: hir::Callee::Closure(_),
                ..
            }
        ),
        "the side-effectful source closure call must remain the exact argument"
    );
    assert_eq!(
        closure_call.node.local_index(),
        generic_call.node.local_index() + 1,
        "generic inference must reuse the first lowered argument instead of leaving a discarded HIR node"
    );

    let mut second_session = CompilerSession::default();
    let second_compile = second_session.compile_program(program());
    assert!(second_compile.is_ok(), "repeat generic compilation");
    let Ok(_) = second_compile else {
        return;
    };
    let second_file = second_session.database().active_files().first().copied();
    assert!(
        second_file.is_some(),
        "compiled program must retain its source file"
    );
    let Some(second_file) = second_file else {
        return;
    };
    let second_analysis = second_session.database().analyze_file(second_file);
    assert!(second_analysis.is_ok(), "reanalyze generic source");
    let Ok(second_analysis) = second_analysis else {
        return;
    };
    let second_main_id = second_analysis
        .functions()
        .iter()
        .find(|function| function.name() == "main")
        .map(|function| function.id());
    assert!(second_main_id.is_some(), "repeat main function");
    let Some(second_main_id) = second_main_id else {
        return;
    };
    let second_main = second_session
        .database()
        .typed_hir(second_file, second_main_id);
    assert!(second_main.is_ok(), "repeat typed generic HIR");
    let Ok(second_main) = second_main else {
        return;
    };
    assert_eq!(
        fingerprint::hir_function(main),
        fingerprint::hir_function(second_main.function()),
        "single-pass generic argument lowering must be fingerprint-deterministic"
    );
}

#[test]
fn generic_variadic_calls_pack_each_lowered_argument_once() {
    let run = compile_and_run(
        r#"
            package main

            func total[T ~int](values ...T) T {
                var result T
                for _, value := range values {
                    result += value
                }
                return result
            }

            func main() {
                println(total(1, 2, 3))
                println(total[int]())
            }
        "#,
    );

    assert_eq!(run.stderr, b"6\n0\n");
}

#[test]
fn explicit_and_partial_instantiations_bind_as_non_escaping_function_values() {
    let run = compile_and_run(
        r#"
            package main

            func add[T ~int](left, right T) T { return left + right }
            func head[S ~[]E, E any](values S) E { return values[0] }

            func main() {
                addInts := add[int]
                intHead := head[[]int]
                println(addInts(2, 3), intHead([]int{7, 8}))
            }
        "#,
    );

    assert_eq!(run.stderr, b"5 7\n");
}

#[test]
fn channel_constraints_infer_element_types_from_channel_arguments() {
    let run = compile_and_run(
        r#"
            package main

            func recv[C ~chan E | ~<-chan E, E any](channel C) E { return <-channel }

            func main() {
                channel := make(chan int, 2)
                channel <- 5
                channel <- 6
                println(recv(channel))
                var receiveOnly <-chan int = channel
                println(recv(receiveOnly))
            }
        "#,
    );

    assert_eq!(run.stderr, b"5\n6\n");
}

#[test]
fn instantiated_named_constraints_project_inference_through_their_type_arguments() {
    let run = compile_and_run(
        r#"
            package main

            type SliceOf[Item any] interface { ~[]Item }

            func head[S SliceOf[E], E any](values S) E { return values[0] }
            func index[S SliceOf[E], E comparable](values S, target E) int {
                for index, value := range values {
                    if value == target { return index }
                }
                return -1
            }

            func main() {
                values := []int{3, 5, 8}
                println(head(values), index(values, 5), index(values, 13))
            }
        "#,
    );

    assert_eq!(run.stderr, b"3 1 -1\n");
}

#[test]
fn generic_method_receivers_map_renamed_and_blank_type_parameters_positionally() {
    let run = compile_and_run(
        r#"
            package main

            type Pair[A, B any] struct {
                First A
                Second B
            }

            func (pair Pair[X, Y]) Swap() Pair[Y, X] {
                return Pair[Y, X]{First: pair.Second, Second: pair.First}
            }

            func (pair Pair[Head, _]) Head() Head { return pair.First }

            func main() {
                pair := Pair[int, string]{First: 7, Second: "seven"}
                swapped := pair.Swap()
                println(swapped.First, swapped.Second, pair.Head())
            }
        "#,
    );

    assert_eq!(run.stderr, b"seven 7 7\n");
}

#[test]
fn omitted_blank_and_defined_slice_receiver_forms_lower_without_bindings() {
    let run = compile_and_run(
        r#"
            package main

            type Count int
            type Values []int

            func (Count) Unit() string { return "items" }
            func (_ Count) Kind() string { return "count" }
            func (values Values) First() int { return values[0] }

            func main() {
                value := Count(2)
                println(value.Unit(), value.Kind(), Values{7, 8}.First())
            }
        "#,
    );

    assert_eq!(run.stderr, b"items count 7\n");
}

#[test]
fn generic_aliases_substitute_to_their_target_types() {
    let run = compile_and_run(
        r#"
            package main

            type Vector[T any] = []T
            type Pair[A, B any] struct {
                First A
                Second B
            }
            type IntPair[B any] = Pair[int, B]

            func main() {
                var values Vector[int] = []int{1, 2, 3}
                values = append(values, 4)
                pair := IntPair[string]{First: 1, Second: "two"}
                var same Pair[int, string] = pair
                println(len(values), values[3], same.First, same.Second)
            }
        "#,
    );

    assert_eq!(run.stderr, b"4 4 1 two\n");
}

#[test]
fn inference_preserves_defined_types_over_identical_unnamed_types() {
    let run = compile_and_run(
        r#"
            package main

            type Values []int

            func (values Values) Tag() string { return "Values" }
            func keep[S ~[]E, E any](values S) S { return values }
            func choose[T any](first, second T) T { return second }

            func main() {
                kept := keep(Values{1, 2})
                first := choose(Values{9}, []int{4, 5})
                second := choose([]int{4, 5}, Values{9})
                println(kept.Tag(), first.Tag(), second.Tag(), first[0], second[0])
            }
        "#,
    );

    assert_eq!(run.stderr, b"Values Values Values 4 9\n");
}

#[test]
fn instantiated_defined_and_unnamed_structs_are_mutually_assignable() {
    let run = compile_and_run(
        r#"
            package main

            type Pair[A, B any] struct {
                First A
                Second B
            }

            func main() {
                literal := struct {
                    First int
                    Second string
                }{First: 1, Second: "one"}
                var pair Pair[int, string] = literal
                var back struct {
                    First int
                    Second string
                } = pair
                pair.First = 2
                println(pair.First, pair.Second, back.First, back.Second, literal.First)
            }
        "#,
    );

    assert_eq!(run.stderr, b"2 one 1 one 1\n");
}

#[test]
fn tilde_only_constraints_do_not_infer_an_absent_type_argument() {
    let error = compile_program(raw_program(
        "generics.go",
        "/checkout/generics.go",
        "package main\nfunc makeValue[P ~int]() P { return 1 }\nfunc main() { _ = makeValue() }\n",
    ))
    .err();
    assert!(
        error.is_some(),
        "a tilde term does not supply an absent type argument"
    );
    let Some(error) = error else {
        return;
    };

    let diagnostic = error.diagnostics().first();
    assert!(diagnostic.is_some(), "missing generic inference diagnostic");
    let Some(diagnostic) = diagnostic else {
        return;
    };
    assert_eq!(diagnostic.code, "GORS2002");
    assert!(
        diagnostic
            .message
            .contains("cannot infer type parameters P")
    );
    assert_eq!(diagnostic.file, "/checkout/generics.go");
}

#[test]
fn generic_aliases_cannot_be_method_receiver_bases() {
    let error = compile_program(raw_program(
        "generics.go",
        "/checkout/generics.go",
        "package main\ntype Values[T any] = []T\nfunc (values Values[T]) Len() int { return len(values) }\nfunc main() { _ = Values[int]{1}.Len() }\n",
    ))
    .err();
    assert!(
        error.is_some(),
        "an alias must not become a method receiver base"
    );
    let Some(error) = error else {
        return;
    };

    let diagnostic = error.diagnostics().first();
    assert!(diagnostic.is_some(), "missing generic alias diagnostic");
    let Some(diagnostic) = diagnostic else {
        return;
    };
    assert_eq!(diagnostic.code, "GORS2002");
    assert!(
        diagnostic
            .message
            .contains("must be a defined type, not an alias")
    );
    assert_eq!(diagnostic.file, "/checkout/generics.go");
}
