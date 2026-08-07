//! Address-taken local discovery and storage planning.

use std::collections::{BTreeMap, BTreeSet};

use super::super::super::LocalDecl;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::types::{IntTy, Ty};

pub(in crate::compiler::mir::lower) fn plan_addressed_locals(
    function: &hir::Function,
    locals: &mut Vec<LocalDecl>,
) -> Result<BTreeMap<LocalId, LocalId>, Diagnostic> {
    let mut addressed = BTreeSet::new();
    collect_block_addresses(&function.body, &mut addressed);
    for closure in &function.closures {
        collect_block_addresses(&closure.body, &mut addressed);
    }

    let mut plan = BTreeMap::new();
    for local in addressed {
        let declaration = locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("addressed unknown local {}", local.0)))?;
        if declaration.ty.underlying() != &Ty::Int(IntTy::Int)
            && declaration.ty.bootstrap_i64_struct_fields().is_none()
            && !declaration
                .ty
                .uses_interface_aggregate_pointer_representation()
        {
            return Err(Diagnostic::backend(format!(
                "addressed local {} has unsupported type {:?}",
                local.0, declaration.ty
            )));
        }
        let pointer_id = LocalId(
            u32::try_from(locals.len())
                .map_err(|_| Diagnostic::backend("function exceeds the MIR local ID space"))?,
        );
        locals.push(LocalDecl {
            id: pointer_id,
            name: None,
            ty: Ty::Pointer(Box::new(declaration.ty.clone())),
            kind: hir::LocalKind::Temporary,
        });
        plan.insert(local, pointer_id);
    }
    Ok(plan)
}

fn collect_block_addresses(block: &hir::Block, addressed: &mut BTreeSet<LocalId>) {
    for statement in &block.stmts {
        collect_statement_addresses(statement, addressed);
    }
}

fn collect_statement_addresses(statement: &hir::Stmt, addressed: &mut BTreeSet<LocalId>) {
    match &statement.kind {
        hir::StmtKind::Let { values, .. } => {
            collect_expression_addresses(values, addressed);
        }
        hir::StmtKind::Assign {
            destinations,
            values,
            ..
        } => {
            collect_assignment_target_addresses(destinations, addressed);
            collect_expression_addresses(values, addressed);
        }
        hir::StmtKind::LetTuple { value, .. } => {
            collect_expr_addresses(value, addressed);
        }
        hir::StmtKind::AssignTuple {
            destinations,
            value,
            ..
        } => {
            collect_assignment_target_addresses(destinations, addressed);
            collect_expr_addresses(value, addressed);
        }
        hir::StmtKind::Expr(expression) => collect_expr_addresses(expression, addressed),
        hir::StmtKind::ClosureBinding(_) => {}
        hir::StmtKind::Defer { values, body, .. } | hir::StmtKind::Go { values, body, .. } => {
            collect_expression_addresses(values, addressed);
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Return(values) => collect_expression_addresses(values, addressed),
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                collect_statement_addresses(init, addressed);
            }
            collect_expr_addresses(condition, addressed);
            collect_block_addresses(then_block, addressed);
            if let Some(else_branch) = else_branch {
                collect_statement_addresses(else_branch, addressed);
            }
        }
        hir::StmtKind::For {
            init,
            condition,
            post,
            body,
            ..
        } => {
            if let Some(init) = init {
                collect_statement_addresses(init, addressed);
            }
            if let Some(condition) = condition {
                collect_expr_addresses(condition, addressed);
            }
            if let Some(post) = post {
                collect_statement_addresses(post, addressed);
            }
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Range {
            bindings,
            expression,
            body,
            ..
        } => {
            if let hir::RangeBindings::Assigned { targets, .. } = bindings {
                collect_assignment_target_addresses(targets, addressed);
            }
            collect_expr_addresses(expression, addressed);
            collect_block_addresses(body, addressed);
        }
        hir::StmtKind::Block(block) | hir::StmtKind::Breakable { body: block, .. } => {
            collect_block_addresses(block, addressed);
        }
        hir::StmtKind::Label { statement, .. } => {
            if let Some(statement) = statement {
                collect_statement_addresses(statement, addressed);
            }
        }
        hir::StmtKind::Goto(_) | hir::StmtKind::Break(_) | hir::StmtKind::Continue(_) => {}
    }
}

fn collect_assignment_target_addresses(
    targets: &[hir::AssignTarget],
    addressed: &mut BTreeSet<LocalId>,
) {
    for target in targets {
        match &target.kind {
            hir::AssignTargetKind::SliceIndex { slice, index, .. } => {
                collect_expr_addresses(slice, addressed);
                collect_expr_addresses(index, addressed);
            }
            hir::AssignTargetKind::MapIndex { map, key } => {
                collect_expr_addresses(map, addressed);
                collect_expr_addresses(key, addressed);
            }
            hir::AssignTargetKind::ArrayIndex { index, .. } => {
                collect_expr_addresses(index, addressed);
            }
            hir::AssignTargetKind::Pointer { pointer, .. }
            | hir::AssignTargetKind::PointerStructField { pointer, .. } => {
                collect_expr_addresses(pointer, addressed);
            }
            hir::AssignTargetKind::Local(_)
            | hir::AssignTargetKind::Discard
            | hir::AssignTargetKind::StructFieldPath { .. } => {}
        }
    }
}

fn collect_expression_addresses<'a>(
    expressions: impl IntoIterator<Item = &'a hir::Expr>,
    addressed: &mut BTreeSet<LocalId>,
) {
    for expression in expressions {
        collect_expr_addresses(expression, addressed);
    }
}

fn collect_expr_addresses(expression: &hir::Expr, addressed: &mut BTreeSet<LocalId>) {
    match &expression.kind {
        hir::ExprKind::AddressOfLocal(local) => {
            addressed.insert(*local);
        }
        hir::ExprKind::AddressOfValue(value)
        | hir::ExprKind::PointerStructValue(value)
        | hir::ExprKind::InterfaceValue { value, .. } => {
            collect_expr_addresses(value, addressed);
        }
        hir::ExprKind::MethodReceiver { receiver, plan } => {
            if plan.adjustment == hir::MethodReceiverAdjustment::AutoAddress
                && let hir::ExprKind::Local(local) = receiver.kind
            {
                addressed.insert(local);
            }
            collect_expr_addresses(receiver, addressed);
        }
        hir::ExprKind::InterfaceCall { receiver, args, .. } => {
            collect_expr_addresses(receiver, addressed);
            collect_expression_addresses(args, addressed);
        }
        hir::ExprKind::Binary { left, right, .. } => {
            collect_expr_addresses(left, addressed);
            collect_expr_addresses(right, addressed);
        }
        hir::ExprKind::Unary { operand, .. } => collect_expr_addresses(operand, addressed),
        hir::ExprKind::Conversion { value } => collect_expr_addresses(value, addressed),
        hir::ExprKind::ArrayIndexI64 { array, index } => {
            collect_expr_addresses(array, addressed);
            collect_expr_addresses(index, addressed);
        }
        hir::ExprKind::ArrayIndex { array, index } => {
            collect_expr_addresses(array, addressed);
            collect_expr_addresses(index, addressed);
        }
        hir::ExprKind::AggregateSliceIndex { slice, index, .. } => {
            collect_expr_addresses(slice, addressed);
            collect_expr_addresses(index, addressed);
        }
        hir::ExprKind::Append { slice, arguments } => {
            collect_expr_addresses(slice, addressed);
            match arguments {
                hir::AppendArguments::Elements(elements) => {
                    collect_expression_addresses(elements, addressed);
                }
                hir::AppendArguments::Spread(value) => {
                    collect_expr_addresses(value, addressed);
                }
            }
        }
        hir::ExprKind::AggregateMapIndex { map, key, .. } => {
            collect_expr_addresses(map, addressed);
            collect_expr_addresses(key, addressed);
        }
        hir::ExprKind::ArrayLen { array, .. } => collect_expr_addresses(array, addressed),
        hir::ExprKind::DynamicSliceLiteralI64(elements)
        | hir::ExprKind::SliceLiteralGoString(elements)
        | hir::ExprKind::AggregateSliceLiteral { elements, .. } => {
            collect_expression_addresses(elements, addressed);
        }
        hir::ExprKind::StructLiteral(fields) => collect_expression_addresses(fields, addressed),
        hir::ExprKind::ArrayLiteral(elements) => {
            for (_, element) in elements {
                collect_expr_addresses(element, addressed);
            }
        }
        hir::ExprKind::StructField { structure, .. } => {
            collect_expr_addresses(structure, addressed);
        }
        hir::ExprKind::MapLiteralStringI64(entries)
        | hir::ExprKind::MapLiteralI64GoString(entries)
        | hir::ExprKind::AggregateMapLiteral { entries, .. } => {
            for (key, value) in entries {
                collect_expr_addresses(key, addressed);
                collect_expr_addresses(value, addressed);
            }
        }
        hir::ExprKind::Call { args, .. } => collect_expression_addresses(args, addressed),
        hir::ExprKind::ForwardedCall {
            prefix,
            source_call,
            ..
        } => {
            collect_expression_addresses(prefix, addressed);
            collect_expr_addresses(source_call, addressed);
        }
        hir::ExprKind::Constant(_)
        | hir::ExprKind::Local(_)
        | hir::ExprKind::GlobalConstant(..)
        | hir::ExprKind::GlobalVariable(..)
        | hir::ExprKind::Recover
        | hir::ExprKind::SliceLiteralI64(_)
        | hir::ExprKind::SliceLiteralU8(_)
        | hir::ExprKind::SliceLiteralBool(_)
        | hir::ExprKind::ArrayLiteralI64(_) => {}
    }
}
