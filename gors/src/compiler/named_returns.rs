use std::cell::RefCell;

use syn::Token;

use super::{
    local_binding_rust_name,
    shared_captures::{
        is_shared_capture_name, shared_capture_init_expr, shared_capture_read_expr,
        shared_capture_type,
    },
    syn_inspect::expr_is_ident,
    synthetic_names,
};

thread_local! {
    static NAMED_RETURN_IDENTS: RefCell<Vec<syn::Ident>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn replace_idents(idents: Vec<syn::Ident>) -> Vec<syn::Ident> {
    NAMED_RETURN_IDENTS.with(|current| std::mem::replace(&mut *current.borrow_mut(), idents))
}

pub(super) fn take_idents() -> Vec<syn::Ident> {
    NAMED_RETURN_IDENTS.with(|idents| std::mem::take(&mut *idents.borrow_mut()))
}

pub(super) fn restore_idents(previous: Vec<syn::Ident>) {
    NAMED_RETURN_IDENTS.with(|idents| {
        *idents.borrow_mut() = previous;
    });
}

pub(super) fn is_name(name: &str) -> bool {
    let rust_name = local_binding_rust_name(name);
    NAMED_RETURN_IDENTS.with(|idents| idents.borrow().iter().any(|ident| *ident == rust_name))
}

pub(super) fn current_expr() -> Option<syn::Expr> {
    NAMED_RETURN_IDENTS.with(|idents| {
        let idents = idents.borrow();
        if idents.is_empty() {
            None
        } else {
            Some(named_return_expr(&idents, true))
        }
    })
}

pub(super) fn temp_idents(count: usize) -> Vec<syn::Ident> {
    synthetic_names::next_named_return_temp_idents(count)
}

pub(super) fn assignment_stmt(ident: &syn::Ident, value: syn::Expr) -> Option<syn::Stmt> {
    if expr_is_ident(&value, ident) {
        return None;
    }
    let name = ident.to_string();
    Some(if is_shared_capture_name(&name) {
        syn::parse_quote! { *#ident.lock().unwrap() = #value; }
    } else {
        syn::parse_quote! { #ident = #value; }
    })
}

pub(super) fn wrap_block(
    block: &mut syn::Block,
    named_return_info: &[(syn::Ident, Option<syn::Type>, syn::Expr)],
    named_return_idents: &[syn::Ident],
) {
    let label = synthetic_names::next_named_return_label();
    let mut declarations: Vec<syn::Stmt> = named_return_info
        .iter()
        .map(|(ident, rust_type, zero)| named_return_decl_stmt(ident, rust_type, zero))
        .collect();
    let body_stmts = std::mem::take(&mut block.stmts);
    let mut body = syn::Block {
        brace_token: syn::token::Brace::default(),
        stmts: body_stmts,
    };
    rewrite_returns_to_break(&mut body, named_return_idents, &label);
    let body_stmts = body.stmts;
    let labeled_body: syn::Stmt = syn::parse_quote! { #label: { #(#body_stmts)* }; };
    declarations.push(labeled_body);
    declarations.push(named_return_return_stmt(named_return_idents));
    block.stmts = declarations;
}

fn rewrite_returns_to_break(
    block: &mut syn::Block,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    for stmt in &mut block.stmts {
        rewrite_returns_in_stmt(stmt, named_return_idents, label);
    }
}

fn rewrite_returns_in_stmt(
    stmt: &mut syn::Stmt,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    if let syn::Stmt::Expr(expr, _semi) = stmt {
        rewrite_returns_in_expr(expr, named_return_idents, label);
    }
}

fn rewrite_returns_in_expr(
    expr: &mut syn::Expr,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    match expr {
        syn::Expr::Return(ret) => {
            *expr = named_return_break_expr(ret.expr.take(), named_return_idents, label);
        }
        syn::Expr::Block(block) => {
            rewrite_returns_to_break(&mut block.block, named_return_idents, label);
        }
        syn::Expr::Closure(_) => {}
        syn::Expr::ForLoop(for_expr) => {
            rewrite_returns_to_break(&mut for_expr.body, named_return_idents, label);
        }
        syn::Expr::If(if_expr) => {
            rewrite_returns_to_break(&mut if_expr.then_branch, named_return_idents, label);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_returns_in_expr(else_expr, named_return_idents, label);
            }
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_returns_to_break(&mut loop_expr.body, named_return_idents, label);
        }
        syn::Expr::Match(match_expr) => {
            for arm in &mut match_expr.arms {
                rewrite_returns_in_expr(&mut arm.body, named_return_idents, label);
            }
        }
        syn::Expr::TryBlock(try_block) => {
            rewrite_returns_to_break(&mut try_block.block, named_return_idents, label);
        }
        syn::Expr::While(while_expr) => {
            rewrite_returns_to_break(&mut while_expr.body, named_return_idents, label);
        }
        _ => {}
    }
}

fn named_return_break_expr(
    return_expr: Option<Box<syn::Expr>>,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) -> syn::Expr {
    let mut stmts = Vec::new();
    if let Some(return_expr) = return_expr {
        let return_expr = *return_expr;
        match named_return_idents {
            [] => {}
            [ident] => {
                if let Some(stmt) = assignment_stmt(ident, return_expr) {
                    stmts.push(stmt);
                }
            }
            idents => {
                let temps = temp_idents(idents.len());
                let temp_pats = temps.iter();
                stmts.push(syn::parse_quote! { let (#(#temp_pats),*) = #return_expr; });
                for (ident, temp) in idents.iter().zip(temps) {
                    let value: syn::Expr = syn::parse_quote! { #temp };
                    if let Some(stmt) = assignment_stmt(ident, value) {
                        stmts.push(stmt);
                    }
                }
            }
        }
    }
    let break_stmt: syn::Stmt = syn::parse_quote! { break #label; };
    stmts.push(break_stmt);
    syn::parse_quote! {{ #(#stmts)* }}
}

fn named_return_expr(idents: &[syn::Ident], clone_unshared: bool) -> syn::Expr {
    match idents {
        [] => syn::parse_quote! { () },
        [ident] => named_return_ident_expr(ident, clone_unshared),
        idents => {
            let elems = idents
                .iter()
                .map(|ident| named_return_ident_expr(ident, clone_unshared));
            syn::parse_quote! { (#(#elems),*) }
        }
    }
}

fn named_return_ident_expr(ident: &syn::Ident, clone_unshared: bool) -> syn::Expr {
    let name = ident.to_string();
    if let Some(expr) = shared_capture_read_expr(&name) {
        return expr;
    }
    if clone_unshared {
        syn::parse_quote! { (#ident).clone() }
    } else {
        syn::parse_quote! { #ident }
    }
}

fn named_return_decl_stmt(
    ident: &syn::Ident,
    rust_type: &Option<syn::Type>,
    zero: &syn::Expr,
) -> syn::Stmt {
    let name = ident.to_string();
    let init = shared_capture_init_expr(&name, zero.clone());
    if let Some(rust_type) = rust_type {
        let rust_type = shared_capture_type(&name, rust_type.clone());
        syn::parse_quote! { let mut #ident: #rust_type = #init; }
    } else {
        syn::parse_quote! { let mut #ident = #init; }
    }
}

fn named_return_return_stmt(idents: &[syn::Ident]) -> syn::Stmt {
    let expr = named_return_expr(idents, false);
    syn::Stmt::Expr(
        syn::Expr::Return(syn::ExprReturn {
            attrs: vec![],
            return_token: <Token![return]>::default(),
            expr: Some(Box::new(expr)),
        }),
        None,
    )
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote as rust;

    #[test]
    fn wrap_block_rewrites_explicit_tuple_return_to_named_assignments() {
        super::synthetic_names::reset_lowering_counters();
        let mut block: syn::Block = rust!({
            return (1, 2);
        });
        let left = syn::Ident::new("left", proc_macro2::Span::mixed_site());
        let right = syn::Ident::new("right", proc_macro2::Span::mixed_site());
        let info = vec![
            (
                left.clone(),
                Some(rust!(isize)),
                syn::parse_quote! { Default::default() },
            ),
            (
                right.clone(),
                Some(rust!(isize)),
                syn::parse_quote! { Default::default() },
            ),
        ];

        super::wrap_block(&mut block, &info, &[left, right]);
        let output = quote!(#block).to_string();

        assert!(output.contains("'__gors_named_return_0"));
        assert!(output.contains("__gors_named_return_1_0"));
        assert!(output.contains("break '__gors_named_return_0"));
        assert!(output.contains("return (left , right)"));
    }
}
