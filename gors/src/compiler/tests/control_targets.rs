use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::ids::ControlTargetId;
use crate::compiler::{CompilerSession, compile_file, hir, lower_to_hir};

#[test]
fn hir_resolves_nested_break_and_continue_to_exact_dense_targets() {
    let file = lower_to_hir(
        "targets.go",
        r#"
            package main
            func main() {
                for {
                    switch {
                    default:
                        select {
                        default:
                            break
                        }
                        break
                    }
                    continue
                }
            }
        "#,
    )
    .unwrap();
    let main = file
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let mut regions = Vec::new();
    let mut breaks = Vec::new();
    let mut continues = Vec::new();
    collect_control_targets(&main.body, &mut regions, &mut breaks, &mut continues);

    assert_eq!(
        regions,
        vec![ControlTargetId(0), ControlTargetId(1), ControlTargetId(2)]
    );
    assert_eq!(breaks, vec![ControlTargetId(2), ControlTargetId(1)]);
    assert_eq!(continues, vec![ControlTargetId(0)]);
}

#[test]
fn nested_switch_select_and_loop_branches_reach_only_the_resolved_region() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                total := 0
            Outer:
                for i := 0; i < 4; i++ {
                    switch i {
                    case 0:
                        total++
                        break
                    case 1:
                        inner := 0
                        for {
                            inner++
                            if inner == 2 {
                                break
                            }
                            total += 10
                        }
                        continue Outer
                    case 2:
                    Selected:
                        select {
                        default:
                            total += 100
                            break Selected
                        }
                        total += 1000
                    case 3:
                        break Outer
                    }
                    total += 10000
                }
                if total != 21111 {
                    panic("control target changed")
                }
                println("control-targets: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"control-targets: ok\n");
}

#[test]
fn labeled_expression_and_type_switch_breaks_use_break_only_targets() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                total := 0
            Expression:
                switch 1 {
                case 1:
                    total = 1
                    break Expression
                    total = 90
                default:
                    panic("expression switch default selected")
                }

                var dynamic any = 2
            Typed:
                switch dynamic.(type) {
                case int:
                    total = total*10 + 2
                    break Typed
                    total = 99
                default:
                    panic("type switch default selected")
                }
                if total != 12 {
                    panic("labeled break-only target changed")
                }
                println("labeled-break-only: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"labeled-break-only: ok\n");
}

#[test]
fn blocking_single_case_select_evaluates_operands_once_and_scopes_receive_bindings() {
    let run = compile_and_run(
        r#"
            package main

            func markedChannel(order []int, channel chan int, digit int) chan int {
                order[0] = order[0]*10 + digit
                return channel
            }

            func markedValue(order []int, digit int, value int) int {
                order[0] = order[0]*10 + digit
                return value
            }

            func markedIndex(order []int, digit int) int {
                order[0] = order[0]*10 + digit
                return 0
            }

            func main() {
                order := []int{0}
                sent := make(chan int, 1)
                select {
                case markedChannel(order, sent, 1) <- markedValue(order, 2, 7):
                    if order[0] != 12 {
                        panic("select send operands were not evaluated once in order")
                    }
                }
                if <-sent != 7 {
                    panic("blocking select send changed")
                }

                received := make(chan int, 1)
                received <- 9
                select {
                case value, open := <-markedChannel(order, received, 3):
                    if value != 9 || !open || order[0] != 123 {
                        panic("blocking select receive changed")
                    }
                }

                receivedAgain := make(chan int, 1)
                receivedAgain <- 11
                var assigned int
                var assignedOpen bool
                select {
                case assigned, assignedOpen = <-markedChannel(order, receivedAgain, 4):
                }
                if assigned != 11 || !assignedOpen || order[0] != 1234 {
                    panic("blocking select receive assignment changed")
                }

                order[0] = 0
                dynamicReceived := make(chan int, 1)
                dynamicReceived <- 13
                destination := []int{0}
                select {
                case destination[markedIndex(order, 2)] = <-markedChannel(order, dynamicReceived, 1):
                }
                if destination[0] != 13 || order[0] != 12 {
                    panic("blocking select prepared its receive target before the channel")
                }

                order[0] = 0
                commaOkReceived := make(chan int, 1)
                commaOkReceived <- 17
                close(commaOkReceived)
                values := []int{0}
                statuses := []bool{false}
                select {
                case values[markedIndex(order, 2)], statuses[markedIndex(order, 3)] = <-markedChannel(order, commaOkReceived, 1):
                }
                if values[0] != 17 || !statuses[0] || order[0] != 123 {
                    panic("blocking select comma-ok targets were not prepared after the receive")
                }

                order[0] = 0
                discarded := make(chan int, 1)
                discarded <- 19
                select {
                case _ = <-markedChannel(order, discarded, 1):
                }
                if len(discarded) != 0 || order[0] != 1 {
                    panic("blocking select discard did not commit the receive")
                }
                println("blocking-select: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"blocking-select: ok\n");
    assert!(run.rust.contains("go_channel_i64_send"), "{}", run.rust);
    assert!(run.rust.contains("go_channel_i64_receive"), "{}", run.rust);
    assert!(!run.rust.contains("try_send"), "{}", run.rust);
    assert!(!run.rust.contains("try_receive"), "{}", run.rust);
}

#[test]
fn blocking_select_hir_materializes_receive_before_dynamic_assignment_targets() {
    let file = lower_to_hir(
        "select-order.go",
        r#"
            package main

            func markedChannel(order []int, channel chan int) chan int {
                order[0] = order[0]*10 + 1
                return channel
            }

            func markedIndex(order []int, digit int) int {
                order[0] = order[0]*10 + digit
                return 0
            }

            func main() {
                order := []int{0}
                channel := make(chan int, 1)
                values := []int{0}
                statuses := []bool{false}
                select {
                case values[markedIndex(order, 2)], statuses[markedIndex(order, 3)] = <-markedChannel(order, channel):
                }
            }
        "#,
    )
    .unwrap();
    let main = file
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let body = main
        .body
        .stmts
        .iter()
        .find_map(|statement| match &statement.kind {
            hir::StmtKind::Breakable { body, .. } => Some(body),
            _ => None,
        })
        .expect("select must lower to a breakable region");

    assert_eq!(body.stmts.len(), 2, "{:#?}", body.stmts);
    let (destinations, value, coercions) = body
        .stmts
        .first()
        .and_then(|statement| match &statement.kind {
            hir::StmtKind::LetTuple {
                destinations,
                value,
                coercions,
            } => Some((destinations, value, coercions)),
            _ => None,
        })
        .expect("blocking receive must be materialized first");
    assert_eq!(destinations.len(), 2);
    assert_eq!(coercions.len(), 2);
    assert!(matches!(
        &value.kind,
        hir::ExprKind::Call {
            callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Receive),
            ..
        }
    ));

    let (destinations, values) = body
        .stmts
        .get(1)
        .and_then(|statement| match &statement.kind {
            hir::StmtKind::Assign {
                destinations,
                values,
                ..
            } => Some((destinations, values)),
            _ => None,
        })
        .expect("blocking receive targets must be assigned second");
    assert!(
        destinations.iter().all(|destination| matches!(
            destination.kind,
            hir::AssignTargetKind::SliceIndex { .. }
        ))
    );
    assert!(
        values
            .iter()
            .all(|value| matches!(value.kind, hir::ExprKind::Local(_)))
    );
}

#[test]
fn range_yield_nested_breaks_stay_local_while_continue_controls_the_callback() {
    let run = compile_and_run(
        r#"
            package main

            func values(yield func(int) bool) {
                for i := 0; i < 5; i++ {
                    if !yield(i) {
                        return
                    }
                }
            }

            func main() {
                ready := make(chan int, 1)
                ready <- 1
                seen := 0
                for value := range values {
                    switch value {
                    case 1:
                        break
                    case 2:
                        continue
                    case 3:
                        select {
                        case <-ready:
                            break
                        }
                    }
                    if value == 4 {
                        break
                    }
                    seen = seen*10 + value
                }
                if seen != 13 {
                    panic("range-yield nested target changed")
                }
                println("range-yield-targets: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"range-yield-targets: ok\n");
}

#[test]
fn invalid_branches_and_closure_crossing_are_rejected_before_mir() {
    for (source, expected) in [
        (
            "package main\nfunc main() { break }\n",
            "break does not target an enclosing for, switch, or select",
        ),
        (
            "package main\nfunc main() { switch { default: continue } }\n",
            "continue does not target an enclosing for loop",
        ),
        (
            "package main\nfunc main() { Target: switch { default: continue Target } }\n",
            "continue label Target does not denote a for loop",
        ),
        (
            "package main\nfunc main() { for { break Missing } }\n",
            "break label Missing does not denote an enclosing for, switch, or select",
        ),
        (
            "package main\nfunc main() { for { call := func() { break }; call(); break } }\n",
            "break does not target an enclosing for, switch, or select",
        ),
        (
            "package main\nfunc main() { select {} }\n",
            "an empty select requires scheduler-backed blocking",
        ),
        (
            "package main\nfunc main() { a := make(chan int); b := make(chan int); select { case <-a: case <-b: } }\n",
            "select with multiple communication cases requires channel arbitration",
        ),
    ] {
        let errors = compile_file("invalid-target.go", source)
            .err()
            .expect("invalid branch target must be rejected");
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{errors:?}"
        );
    }
}

#[test]
fn control_target_comment_edits_reuse_all_semantic_products() {
    let before = r#"
        package main
        func main() {
            for {
                switch { default: break }
                break
            }
        }
    "#;
    let after = r#"
        package main
        func main() {
            // Target identities are structural, not physical-source identities.
            for {
                switch { default: break }
                break
            }
        }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    let scheduler = session.scheduler_telemetry();
    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    let telemetry = session.database().telemetry();
    for kind in [
        QueryKind::TypedHir,
        QueryKind::VerifiedGoMir,
        QueryKind::NormalizedGoMir,
        QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 0, "{kind:?}");
    }
    assert_eq!(session.scheduler_telemetry(), scheduler);
}

#[test]
fn control_target_form_edits_invalidate_the_owning_root() {
    let before = r#"
        package main
        func changed() { for { switch { default: break }; break } }
        func untouched() int { return 7 }
        func main() { changed(); println(untouched()) }
    "#;
    let after = r#"
        package main
        func changed() { for { switch { default: continue } } }
        func untouched() int { return 7 }
        func main() { changed(); println(untouched()) }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    let scheduled = session.scheduler_telemetry().scheduled_roots;
    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    assert_eq!(session.scheduler_telemetry().scheduled_roots, scheduled + 1);
    let telemetry = session.database().telemetry();
    for kind in [
        QueryKind::TypedHir,
        QueryKind::VerifiedGoMir,
        QueryKind::NormalizedGoMir,
        QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 1, "{kind:?}");
    }
}

fn collect_control_targets(
    block: &hir::Block,
    regions: &mut Vec<ControlTargetId>,
    breaks: &mut Vec<ControlTargetId>,
    continues: &mut Vec<ControlTargetId>,
) {
    for statement in &block.stmts {
        collect_statement_targets(statement, regions, breaks, continues);
    }
}

fn collect_statement_targets(
    statement: &hir::Stmt,
    regions: &mut Vec<ControlTargetId>,
    breaks: &mut Vec<ControlTargetId>,
    continues: &mut Vec<ControlTargetId>,
) {
    match &statement.kind {
        hir::StmtKind::If {
            init,
            then_block,
            else_branch,
            ..
        } => {
            if let Some(init) = init {
                collect_statement_targets(init, regions, breaks, continues);
            }
            collect_control_targets(then_block, regions, breaks, continues);
            if let Some(else_branch) = else_branch {
                collect_statement_targets(else_branch, regions, breaks, continues);
            }
        }
        hir::StmtKind::For {
            target,
            init,
            post,
            body,
            ..
        } => {
            regions.push(*target);
            if let Some(init) = init {
                collect_statement_targets(init, regions, breaks, continues);
            }
            collect_control_targets(body, regions, breaks, continues);
            if let Some(post) = post {
                collect_statement_targets(post, regions, breaks, continues);
            }
        }
        hir::StmtKind::Range { target, body, .. } | hir::StmtKind::Breakable { target, body } => {
            regions.push(*target);
            collect_control_targets(body, regions, breaks, continues);
        }
        hir::StmtKind::Block(block) => {
            collect_control_targets(block, regions, breaks, continues);
        }
        hir::StmtKind::Label { statement, .. } => {
            if let Some(statement) = statement {
                collect_statement_targets(statement, regions, breaks, continues);
            }
        }
        hir::StmtKind::Defer { body, .. } | hir::StmtKind::Go { body, .. } => {
            collect_control_targets(body, regions, breaks, continues);
        }
        hir::StmtKind::Break(target) => breaks.push(*target),
        hir::StmtKind::Continue(target) => continues.push(*target),
        hir::StmtKind::Let { .. }
        | hir::StmtKind::LetTuple { .. }
        | hir::StmtKind::Assign { .. }
        | hir::StmtKind::AssignTuple { .. }
        | hir::StmtKind::Expr(_)
        | hir::StmtKind::ClosureBinding(_)
        | hir::StmtKind::Return(_)
        | hir::StmtKind::Goto(_) => {}
    }
}
