use super::*;
use crate::compiler::ids::ControlTargetId;

#[test]
fn control_target_identity_and_branch_form_participate_in_hir_fingerprints() {
    let (original, _, _) =
        lower_stages("package main\nfunc main() { for { switch { default: break }; continue } }\n");
    let original_fingerprint = hir_function(hir_named(&original, "main"));

    let mut changed_region = original.clone();
    let target = first_breakable_target_mut(main_mut(&mut changed_region));
    *target = ControlTargetId(target.0 + 17);
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_region, "main"))
    );

    let loop_target = first_loop_target(hir_named(&original, "main"));
    let mut changed_branch_target = original.clone();
    let branch = first_break_mut(main_mut(&mut changed_branch_target));
    *branch = loop_target;
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_branch_target, "main"))
    );

    let mut changed_form = original;
    let branch = first_break_statement_mut(main_mut(&mut changed_form));
    let hir::StmtKind::Break(target) = branch else {
        panic!("expected break statement");
    };
    let target = *target;
    *branch = hir::StmtKind::Continue(target);
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_form, "main"))
    );
}

fn main_mut(file: &mut hir::File) -> &mut hir::Function {
    file.functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main HIR function")
}

fn first_loop_target(function: &hir::Function) -> ControlTargetId {
    function
        .body
        .stmts
        .iter()
        .find_map(|statement| match statement.kind {
            hir::StmtKind::For { target, .. } => Some(target),
            _ => None,
        })
        .expect("loop target")
}

fn first_breakable_target_mut(function: &mut hir::Function) -> &mut ControlTargetId {
    first_breakable_in_block(&mut function.body).expect("breakable target")
}

fn first_breakable_in_block(block: &mut hir::Block) -> Option<&mut ControlTargetId> {
    for statement in &mut block.stmts {
        match &mut statement.kind {
            hir::StmtKind::Breakable { target, .. } => return Some(target),
            hir::StmtKind::If {
                init,
                then_block,
                else_branch,
                ..
            } => {
                if let Some(target) = init.as_deref_mut().and_then(first_breakable_in_statement) {
                    return Some(target);
                }
                if let Some(target) = first_breakable_in_block(then_block) {
                    return Some(target);
                }
                if let Some(target) = else_branch
                    .as_deref_mut()
                    .and_then(first_breakable_in_statement)
                {
                    return Some(target);
                }
            }
            hir::StmtKind::For {
                init, post, body, ..
            } => {
                if let Some(target) = init.as_deref_mut().and_then(first_breakable_in_statement) {
                    return Some(target);
                }
                if let Some(target) = first_breakable_in_block(body) {
                    return Some(target);
                }
                if let Some(target) = post.as_deref_mut().and_then(first_breakable_in_statement) {
                    return Some(target);
                }
            }
            hir::StmtKind::Range { body, .. }
            | hir::StmtKind::Block(body)
            | hir::StmtKind::Defer { body, .. }
            | hir::StmtKind::Go { body, .. } => {
                if let Some(target) = first_breakable_in_block(body) {
                    return Some(target);
                }
            }
            hir::StmtKind::Label { statement, .. } => {
                if let Some(target) = statement
                    .as_deref_mut()
                    .and_then(first_breakable_in_statement)
                {
                    return Some(target);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_breakable_in_statement(statement: &mut hir::Stmt) -> Option<&mut ControlTargetId> {
    match &mut statement.kind {
        hir::StmtKind::Breakable { target, .. } => Some(target),
        hir::StmtKind::Block(block)
        | hir::StmtKind::Defer { body: block, .. }
        | hir::StmtKind::Go { body: block, .. }
        | hir::StmtKind::Range { body: block, .. } => first_breakable_in_block(block),
        hir::StmtKind::For { body, .. } => first_breakable_in_block(body),
        hir::StmtKind::Label { statement, .. } => statement
            .as_deref_mut()
            .and_then(first_breakable_in_statement),
        hir::StmtKind::If {
            then_block,
            else_branch,
            ..
        } => first_breakable_in_block(then_block).or_else(|| {
            else_branch
                .as_deref_mut()
                .and_then(first_breakable_in_statement)
        }),
        _ => None,
    }
}

fn first_break_mut(function: &mut hir::Function) -> &mut ControlTargetId {
    let hir::StmtKind::Break(target) = first_break_statement_mut(function) else {
        panic!("expected break statement");
    };
    target
}

fn first_break_statement_mut(function: &mut hir::Function) -> &mut hir::StmtKind {
    first_break_in_block(&mut function.body).expect("break statement")
}

fn first_break_in_block(block: &mut hir::Block) -> Option<&mut hir::StmtKind> {
    for statement in &mut block.stmts {
        if matches!(statement.kind, hir::StmtKind::Break(_)) {
            return Some(&mut statement.kind);
        }
        let found = match &mut statement.kind {
            hir::StmtKind::If {
                init,
                then_block,
                else_branch,
                ..
            } => init
                .as_deref_mut()
                .and_then(first_break_in_statement)
                .or_else(|| first_break_in_block(then_block))
                .or_else(|| {
                    else_branch
                        .as_deref_mut()
                        .and_then(first_break_in_statement)
                }),
            hir::StmtKind::For {
                init, post, body, ..
            } => init
                .as_deref_mut()
                .and_then(first_break_in_statement)
                .or_else(|| first_break_in_block(body))
                .or_else(|| post.as_deref_mut().and_then(first_break_in_statement)),
            hir::StmtKind::Range { body, .. }
            | hir::StmtKind::Block(body)
            | hir::StmtKind::Breakable { body, .. }
            | hir::StmtKind::Defer { body, .. }
            | hir::StmtKind::Go { body, .. } => first_break_in_block(body),
            hir::StmtKind::Label { statement, .. } => {
                statement.as_deref_mut().and_then(first_break_in_statement)
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn first_break_in_statement(statement: &mut hir::Stmt) -> Option<&mut hir::StmtKind> {
    if matches!(statement.kind, hir::StmtKind::Break(_)) {
        return Some(&mut statement.kind);
    }
    match &mut statement.kind {
        hir::StmtKind::If {
            then_block,
            else_branch,
            ..
        } => first_break_in_block(then_block).or_else(|| {
            else_branch
                .as_deref_mut()
                .and_then(first_break_in_statement)
        }),
        hir::StmtKind::For { body, .. }
        | hir::StmtKind::Range { body, .. }
        | hir::StmtKind::Breakable { body, .. } => first_break_in_block(body),
        hir::StmtKind::Block(block)
        | hir::StmtKind::Defer { body: block, .. }
        | hir::StmtKind::Go { body: block, .. } => first_break_in_block(block),
        hir::StmtKind::Label { statement, .. } => {
            statement.as_deref_mut().and_then(first_break_in_statement)
        }
        _ => None,
    }
}
