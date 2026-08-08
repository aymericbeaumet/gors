use super::*;
use crate::compiler::ids::{BasicBlockId, ControlTargetId};

#[test]
fn lowering_rejects_inactive_and_break_only_branch_targets() {
    let mut unknown = crate::compiler::lower_to_hir(
        "unknown-target.go",
        "package main\nfunc main() { for { break } }\n",
    )
    .unwrap();
    let main = main_hir_mut(&mut unknown);
    *first_break_mut(&mut main.body) = ControlTargetId(u32::MAX);
    let error = crate::compiler::mir::lower::lower_function(main).unwrap_err();
    assert!(
        error.message.contains("inactive HIR control target"),
        "{error:?}"
    );

    let mut mismatched = crate::compiler::lower_to_hir(
        "break-only-target.go",
        "package main\nfunc main() { switch { default: break } }\n",
    )
    .unwrap();
    let main = main_hir_mut(&mut mismatched);
    let statement = first_break_statement_mut(&mut main.body);
    let hir::StmtKind::Break(target) = statement else {
        panic!("expected break statement");
    };
    let target = *target;
    *statement = hir::StmtKind::Continue(target);
    let error = crate::compiler::mir::lower::lower_function(main).unwrap_err();
    assert!(
        error.message.contains("break-only HIR control target"),
        "{error:?}"
    );
}

#[test]
fn lowering_rejects_duplicate_active_control_target_ids() {
    let mut hir = crate::compiler::lower_to_hir(
        "duplicate-target.go",
        "package main\nfunc main() { for { switch { default: break }; break } }\n",
    )
    .unwrap();
    let main = main_hir_mut(&mut hir);
    let loop_target = main
        .body
        .stmts
        .iter()
        .find_map(|statement| match statement.kind {
            hir::StmtKind::For { target, .. } => Some(target),
            _ => None,
        })
        .unwrap();
    *first_breakable_target_mut(&mut main.body) = loop_target;

    let error = crate::compiler::mir::lower::lower_function(main).unwrap_err();
    assert!(
        error
            .message
            .contains("duplicate active HIR control target"),
        "{error:?}"
    );
}

#[test]
fn verifier_rejects_a_corrupt_control_flow_target() {
    let mut file = lower("package main\nfunc main() { for { break } }\n");
    let target = file
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Goto(target) => Some(target),
            _ => None,
        })
        .expect("MIR goto target");
    *target = BasicBlockId(u32::MAX);

    let error = file.verify().unwrap_err();
    assert!(error.message.contains("invalid MIR target"), "{error:?}");
}

fn main_hir_mut(file: &mut hir::File) -> &mut hir::Function {
    file.functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main HIR function")
}

fn first_break_mut(block: &mut hir::Block) -> &mut ControlTargetId {
    let hir::StmtKind::Break(target) = first_break_statement_mut(block) else {
        panic!("expected break statement");
    };
    target
}

fn first_break_statement_mut(block: &mut hir::Block) -> &mut hir::StmtKind {
    for statement in &mut block.stmts {
        if matches!(statement.kind, hir::StmtKind::Break(_)) {
            return &mut statement.kind;
        }
        if let Some(branch) = nested_break_mut(&mut statement.kind) {
            return branch;
        }
    }
    panic!("break statement")
}

fn nested_break_mut(kind: &mut hir::StmtKind) -> Option<&mut hir::StmtKind> {
    match kind {
        hir::StmtKind::If {
            init,
            then_block,
            else_branch,
            ..
        } => {
            if let Some(init) = init
                && matches!(init.kind, hir::StmtKind::Break(_))
            {
                return Some(&mut init.kind);
            }
            find_break_in_block(then_block).or_else(|| {
                else_branch.as_deref_mut().and_then(|branch| {
                    if matches!(branch.kind, hir::StmtKind::Break(_)) {
                        Some(&mut branch.kind)
                    } else {
                        nested_break_mut(&mut branch.kind)
                    }
                })
            })
        }
        hir::StmtKind::For { body, .. }
        | hir::StmtKind::Range { body, .. }
        | hir::StmtKind::Block(body)
        | hir::StmtKind::Breakable { body, .. }
        | hir::StmtKind::Defer { body, .. }
        | hir::StmtKind::Go { body, .. } => find_break_in_block(body),
        hir::StmtKind::Label { statement, .. } => statement.as_deref_mut().and_then(|statement| {
            if matches!(statement.kind, hir::StmtKind::Break(_)) {
                Some(&mut statement.kind)
            } else {
                nested_break_mut(&mut statement.kind)
            }
        }),
        _ => None,
    }
}

fn find_break_in_block(block: &mut hir::Block) -> Option<&mut hir::StmtKind> {
    for statement in &mut block.stmts {
        if matches!(statement.kind, hir::StmtKind::Break(_)) {
            return Some(&mut statement.kind);
        }
        if let Some(branch) = nested_break_mut(&mut statement.kind) {
            return Some(branch);
        }
    }
    None
}

fn first_breakable_target_mut(block: &mut hir::Block) -> &mut ControlTargetId {
    for statement in &mut block.stmts {
        match &mut statement.kind {
            hir::StmtKind::Breakable { target, .. } => return target,
            hir::StmtKind::For { body, .. }
            | hir::StmtKind::Range { body, .. }
            | hir::StmtKind::Block(body)
            | hir::StmtKind::Defer { body, .. }
            | hir::StmtKind::Go { body, .. } => {
                if let Some(target) = find_breakable_target(body) {
                    return target;
                }
            }
            hir::StmtKind::If {
                then_block,
                else_branch,
                ..
            } => {
                if let Some(target) = find_breakable_target(then_block) {
                    return target;
                }
                if let Some(target) = else_branch
                    .as_deref_mut()
                    .and_then(find_breakable_target_in_statement)
                {
                    return target;
                }
            }
            hir::StmtKind::Label { statement, .. } => {
                if let Some(target) = statement
                    .as_deref_mut()
                    .and_then(find_breakable_target_in_statement)
                {
                    return target;
                }
            }
            _ => {}
        }
    }
    panic!("breakable target")
}

fn find_breakable_target(block: &mut hir::Block) -> Option<&mut ControlTargetId> {
    for statement in &mut block.stmts {
        if let Some(target) = find_breakable_target_in_statement(statement) {
            return Some(target);
        }
    }
    None
}

fn find_breakable_target_in_statement(statement: &mut hir::Stmt) -> Option<&mut ControlTargetId> {
    match &mut statement.kind {
        hir::StmtKind::Breakable { target, .. } => Some(target),
        hir::StmtKind::For { body, .. }
        | hir::StmtKind::Range { body, .. }
        | hir::StmtKind::Block(body)
        | hir::StmtKind::Defer { body, .. }
        | hir::StmtKind::Go { body, .. } => find_breakable_target(body),
        hir::StmtKind::If {
            then_block,
            else_branch,
            ..
        } => find_breakable_target(then_block).or_else(|| {
            else_branch
                .as_deref_mut()
                .and_then(find_breakable_target_in_statement)
        }),
        hir::StmtKind::Label { statement, .. } => statement
            .as_deref_mut()
            .and_then(find_breakable_target_in_statement),
        _ => None,
    }
}
