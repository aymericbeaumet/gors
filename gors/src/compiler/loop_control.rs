use syn::Token;

use super::synthetic_names;

pub(super) fn range_for_loop(
    pat: syn::Pat,
    iter_expr: syn::Expr,
    mut body: syn::Block,
) -> Vec<syn::Stmt> {
    let label = if has_continue_for_post(&body.stmts, None, true) {
        let label = synthetic_names::next_loop_body_label();
        rewrite_unlabeled_continue_to_label(&mut body.stmts, &label);
        Some(syn::Label {
            name: label,
            colon_token: <Token![:]>::default(),
        })
    } else {
        None
    };
    vec![syn::Stmt::Expr(
        syn::Expr::ForLoop(syn::ExprForLoop {
            attrs: vec![],
            label,
            for_token: <Token![for]>::default(),
            pat: Box::new(pat),
            in_token: <Token![in]>::default(),
            expr: Box::new(iter_expr),
            body,
        }),
        None,
    )]
}

pub(super) fn with_iteration_tail(
    mut body: syn::Block,
    loop_label_name: Option<&str>,
    per_iteration_stmts: Vec<syn::Stmt>,
    post_stmts: Vec<syn::Stmt>,
) -> syn::Block {
    if per_iteration_stmts.is_empty() && post_stmts.is_empty() {
        return body;
    }

    if has_continue_for_post(&body.stmts, loop_label_name, true) {
        // Go runs these statements before the next iteration, including
        // `continue label` targeting this loop. Rust `continue` jumps straight
        // to the loop condition, so route matching continues through a body
        // block and emit the tail statements after that block.
        let body_label = synthetic_names::next_loop_body_label();
        rewrite_continue_for_post(&mut body.stmts, loop_label_name, true, &body_label);

        let labeled_body = syn::Stmt::Expr(
            syn::Expr::Block(syn::ExprBlock {
                attrs: vec![],
                label: Some(syn::Label {
                    name: body_label,
                    colon_token: <Token![:]>::default(),
                }),
                block: body,
            }),
            Some(<Token![;]>::default()),
        );

        let mut loop_stmts = vec![labeled_body];
        loop_stmts.extend(per_iteration_stmts);
        loop_stmts.extend(post_stmts);

        return syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: loop_stmts,
        };
    }

    body.stmts.extend(per_iteration_stmts);
    body.stmts.extend(post_stmts);
    body
}

fn has_continue_for_post(
    stmts: &[syn::Stmt],
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    stmts.iter().any(|stmt| match stmt {
        syn::Stmt::Expr(syn::Expr::Continue(cont), _) => {
            continue_targets_current_loop(cont, loop_label, allow_unlabeled)
        }
        syn::Stmt::Expr(expr, _) => {
            has_continue_for_post_in_expr(expr, loop_label, allow_unlabeled)
        }
        _ => false,
    })
}

fn has_continue_for_post_in_expr(
    expr: &syn::Expr,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    match expr {
        syn::Expr::If(if_expr) => {
            has_continue_for_post(&if_expr.then_branch.stmts, loop_label, allow_unlabeled)
                || if_expr.else_branch.as_ref().is_some_and(|(_, e)| {
                    has_continue_for_post_in_expr(e, loop_label, allow_unlabeled)
                })
        }
        syn::Expr::Block(block) => {
            has_continue_for_post(&block.block.stmts, loop_label, allow_unlabeled)
        }
        syn::Expr::While(while_expr) => has_continue_for_post_in_nested_loop(
            while_expr.label.as_ref(),
            &while_expr.body.stmts,
            loop_label,
        ),
        syn::Expr::Loop(loop_expr) => has_continue_for_post_in_nested_loop(
            loop_expr.label.as_ref(),
            &loop_expr.body.stmts,
            loop_label,
        ),
        syn::Expr::ForLoop(for_loop) => has_continue_for_post_in_nested_loop(
            for_loop.label.as_ref(),
            &for_loop.body.stmts,
            loop_label,
        ),
        _ => false,
    }
}

fn has_continue_for_post_in_nested_loop(
    nested_label: Option<&syn::Label>,
    stmts: &[syn::Stmt],
    loop_label: Option<&str>,
) -> bool {
    let Some(loop_label) = loop_label else {
        return false;
    };
    if nested_label.is_some_and(|label| label.name.ident == loop_label) {
        return false;
    }
    has_continue_for_post(stmts, Some(loop_label), false)
}

fn rewrite_continue_for_post(
    stmts: &mut [syn::Stmt],
    loop_label: Option<&str>,
    allow_unlabeled: bool,
    body_label: &syn::Lifetime,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Continue(cont), semi)
                if continue_targets_current_loop(cont, loop_label, allow_unlabeled) =>
            {
                *stmt = syn::Stmt::Expr(
                    syn::Expr::Break(syn::ExprBreak {
                        attrs: vec![],
                        break_token: <Token![break]>::default(),
                        label: Some(body_label.clone()),
                        expr: None,
                    }),
                    *semi,
                );
            }
            syn::Stmt::Expr(expr, _) => {
                rewrite_continue_for_post_in_expr(expr, loop_label, allow_unlabeled, body_label);
            }
            _ => {}
        }
    }
}

fn rewrite_continue_for_post_in_expr(
    expr: &mut syn::Expr,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
    body_label: &syn::Lifetime,
) {
    match expr {
        syn::Expr::If(if_expr) => {
            rewrite_continue_for_post(
                &mut if_expr.then_branch.stmts,
                loop_label,
                allow_unlabeled,
                body_label,
            );
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_continue_for_post_in_expr(
                    else_expr,
                    loop_label,
                    allow_unlabeled,
                    body_label,
                );
            }
        }
        syn::Expr::Block(block) => {
            rewrite_continue_for_post(
                &mut block.block.stmts,
                loop_label,
                allow_unlabeled,
                body_label,
            );
        }
        syn::Expr::While(while_expr) => {
            rewrite_continue_for_post_in_nested_loop(
                while_expr.label.as_ref(),
                &mut while_expr.body.stmts,
                loop_label,
                body_label,
            );
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_continue_for_post_in_nested_loop(
                loop_expr.label.as_ref(),
                &mut loop_expr.body.stmts,
                loop_label,
                body_label,
            );
        }
        syn::Expr::ForLoop(for_loop) => {
            rewrite_continue_for_post_in_nested_loop(
                for_loop.label.as_ref(),
                &mut for_loop.body.stmts,
                loop_label,
                body_label,
            );
        }
        _ => {}
    }
}

fn rewrite_continue_for_post_in_nested_loop(
    nested_label: Option<&syn::Label>,
    stmts: &mut [syn::Stmt],
    loop_label: Option<&str>,
    body_label: &syn::Lifetime,
) {
    let Some(loop_label) = loop_label else {
        return;
    };
    if nested_label.is_some_and(|label| label.name.ident == loop_label) {
        return;
    }
    rewrite_continue_for_post(stmts, Some(loop_label), false, body_label);
}

fn rewrite_unlabeled_continue_to_label(stmts: &mut [syn::Stmt], loop_label: &syn::Lifetime) {
    for stmt in stmts.iter_mut() {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Continue(cont), _) if cont.label.is_none() => {
                cont.label = Some(loop_label.clone());
            }
            syn::Stmt::Expr(expr, _) => {
                rewrite_unlabeled_continue_to_label_in_expr(expr, loop_label);
            }
            _ => {}
        }
    }
}

fn rewrite_unlabeled_continue_to_label_in_expr(expr: &mut syn::Expr, loop_label: &syn::Lifetime) {
    match expr {
        syn::Expr::If(if_expr) => {
            rewrite_unlabeled_continue_to_label(&mut if_expr.then_branch.stmts, loop_label);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_unlabeled_continue_to_label_in_expr(else_expr, loop_label);
            }
        }
        syn::Expr::Block(block) => {
            rewrite_unlabeled_continue_to_label(&mut block.block.stmts, loop_label);
        }
        syn::Expr::While(_) | syn::Expr::Loop(_) | syn::Expr::ForLoop(_) => {}
        _ => {}
    }
}

fn continue_targets_current_loop(
    cont: &syn::ExprContinue,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    if allow_unlabeled && cont.label.is_none() {
        return true;
    }
    loop_label.is_some_and(|label| cont.label.as_ref().is_some_and(|cont| cont.ident == label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    #[test]
    fn iteration_tail_routes_continue_before_post_statements() {
        synthetic_names::reset_lowering_counters();
        let body: syn::Block = syn::parse_quote!({
            if ready {
                continue;
            }
            seen += 1;
        });
        let per_iteration: syn::Stmt = syn::parse_quote! { capture = capture.clone(); };
        let post: syn::Stmt = syn::parse_quote! { i += 1; };

        let rewritten = with_iteration_tail(body, None, vec![per_iteration], vec![post]);
        let tokens = rewritten.to_token_stream().to_string();

        assert!(tokens.contains("'__gors_loop_body_0"), "{tokens}");
        assert!(tokens.contains("break '__gors_loop_body_0"), "{tokens}");
        assert!(tokens.contains("capture = capture . clone ()"), "{tokens}");
        assert!(tokens.contains("i += 1"), "{tokens}");
    }

    #[test]
    fn range_for_loop_labels_unlabeled_continue_only() {
        synthetic_names::reset_lowering_counters();
        let pat: syn::Pat = syn::parse_quote! { value };
        let iter_expr: syn::Expr = syn::parse_quote! { values };
        let body: syn::Block = syn::parse_quote!({
            continue;
        });

        let stmts = range_for_loop(pat, iter_expr, body);
        let tokens = quote::quote! { #(#stmts)* }.to_string();

        assert!(tokens.contains("'__gors_loop_body_0"), "{tokens}");
        assert!(tokens.contains("continue '__gors_loop_body_0"), "{tokens}");
    }
}
