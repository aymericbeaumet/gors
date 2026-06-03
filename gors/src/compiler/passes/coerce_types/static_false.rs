pub(super) fn prune_branches(stmts: &mut Vec<syn::Stmt>) {
    let mut false_names = std::collections::HashSet::new();
    prune_branches_with(stmts, &mut false_names);
}

fn prune_branches_with(
    stmts: &mut Vec<syn::Stmt>,
    false_names: &mut std::collections::HashSet<String>,
) {
    let old_stmts = std::mem::take(stmts);
    *stmts = old_stmts
        .into_iter()
        .filter_map(|stmt| prune_stmt(stmt, false_names))
        .collect();
}

fn prune_stmt(
    stmt: syn::Stmt,
    false_names: &mut std::collections::HashSet<String>,
) -> Option<syn::Stmt> {
    match stmt {
        syn::Stmt::Local(local) => {
            collect_false_local_names(&local, false_names);
            Some(syn::Stmt::Local(local))
        }
        syn::Stmt::Expr(expr, semi) => {
            prune_expr(expr, false_names).map(|expr| syn::Stmt::Expr(expr, semi))
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => Some(stmt),
    }
}

fn prune_expr(
    expr: syn::Expr,
    false_names: &mut std::collections::HashSet<String>,
) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Block(mut expr_block) => {
            let mut scoped_false_names = false_names.clone();
            prune_branches_with(&mut expr_block.block.stmts, &mut scoped_false_names);
            Some(syn::Expr::Block(expr_block))
        }
        syn::Expr::If(mut expr_if) => {
            if condition_is_static_false(&expr_if.cond, false_names) {
                return expr_if
                    .else_branch
                    .and_then(|(_, else_expr)| prune_expr(*else_expr, false_names));
            }
            let mut then_false_names = false_names.clone();
            prune_branches_with(&mut expr_if.then_branch.stmts, &mut then_false_names);
            expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
                prune_expr(*else_expr, false_names).map(|expr| (else_token, Box::new(expr)))
            });
            Some(syn::Expr::If(expr_if))
        }
        other => Some(other),
    }
}

fn condition_is_static_false(
    expr: &syn::Expr,
    false_names: &std::collections::HashSet<String>,
) -> bool {
    if is_false_lit_expr(expr) {
        return true;
    }
    super::path_ident_name(expr).is_some_and(|name| false_names.contains(&name))
}

fn collect_false_local_names(
    local: &syn::Local,
    false_names: &mut std::collections::HashSet<String>,
) {
    let Some(init) = &local.init else {
        return;
    };
    collect_false_bindings(&local.pat, &init.expr, false_names);
}

fn collect_false_bindings(
    pat: &syn::Pat,
    expr: &syn::Expr,
    false_names: &mut std::collections::HashSet<String>,
) {
    match (pat, expr) {
        (syn::Pat::Ident(pat_ident), expr) if is_false_lit_expr(expr) => {
            false_names.insert(pat_ident.ident.to_string());
        }
        (syn::Pat::Tuple(pat_tuple), syn::Expr::Tuple(expr_tuple)) => {
            for (pat, expr) in pat_tuple.elems.iter().zip(&expr_tuple.elems) {
                collect_false_bindings(pat, expr, false_names);
            }
        }
        (syn::Pat::Type(pat_type), expr) => {
            collect_false_bindings(&pat_type.pat, expr, false_names);
        }
        _ => {}
    }
}

fn is_false_lit_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(lit),
            ..
        }) if !lit.value
    )
}
