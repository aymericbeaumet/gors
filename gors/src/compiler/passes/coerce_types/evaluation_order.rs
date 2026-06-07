use syn::visit_mut::{self, VisitMut};

use super::super::super::{
    syn_inspect::{expr_contains_path_ident, mut_borrowed_path_name, receiver_root_ident_name},
    synthetic_names,
};

pub(super) fn hoist_args_read_after_mut_borrow(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::Call(call), _) = stmt else {
        return Vec::new();
    };

    let mut hoisted = Vec::new();
    hoist_args_read_after_mut_borrow_in_args(&mut call.args, &mut hoisted);
    hoisted
}

pub(super) fn hoist_condition_args_read_after_mut_borrow(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    match stmt {
        syn::Stmt::Expr(syn::Expr::If(if_expr), _) => {
            hoist_args_read_after_mut_borrow_in_expr(&mut if_expr.cond)
        }
        syn::Stmt::Expr(syn::Expr::While(while_expr), _) => {
            hoist_args_read_after_mut_borrow_in_expr(&mut while_expr.cond)
        }
        _ => Vec::new(),
    }
}

fn hoist_args_read_after_mut_borrow_in_expr(expr: &mut syn::Expr) -> Vec<syn::Stmt> {
    struct Hoister {
        hoisted: Vec<syn::Stmt>,
    }

    impl VisitMut for Hoister {
        fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
            visit_mut::visit_expr_call_mut(self, call);
            hoist_args_read_after_mut_borrow_in_args(&mut call.args, &mut self.hoisted);
        }
    }

    let mut hoister = Hoister {
        hoisted: Vec::new(),
    };
    hoister.visit_expr_mut(expr);
    hoister.hoisted
}

fn hoist_args_read_after_mut_borrow_in_args(
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    hoisted: &mut Vec<syn::Stmt>,
) {
    let borrowed: Vec<(usize, String)> = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| mut_borrowed_path_name(arg).map(|name| (index, name)))
        .collect();
    if borrowed.is_empty() {
        return;
    }

    for (borrow_index, name) in borrowed {
        for (arg_index, arg) in args.iter_mut().enumerate() {
            if arg_index <= borrow_index || !expr_contains_path_ident(arg, &name) {
                continue;
            }
            let temp = synthetic_names::preborrow_arg_ident(hoisted.len());
            let value = arg.clone();
            *arg = syn::parse_quote! { #temp };
            hoisted.push(syn::parse_quote! {
                let #temp = #value;
            });
        }
    }
}

pub(super) fn hoist_method_args_read_receiver(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::MethodCall(call), _) = stmt else {
        return Vec::new();
    };
    let Some(receiver_name) = receiver_root_ident_name(&call.receiver) else {
        return Vec::new();
    };

    let mut hoisted = Vec::new();
    for arg in &mut call.args {
        if !expr_contains_path_ident(arg, &receiver_name) {
            continue;
        }
        let temp = synthetic_names::premethod_arg_ident(hoisted.len());
        let value = arg.clone();
        *arg = if expr_is_mut_reference(&value) {
            syn::parse_quote! { &mut *#temp }
        } else {
            syn::parse_quote! { #temp }
        };
        hoisted.push(syn::parse_quote! {
            let #temp = #value;
        });
    }
    hoisted
}

fn expr_is_mut_reference(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Group(group) => expr_is_mut_reference(&group.expr),
        syn::Expr::Paren(paren) => expr_is_mut_reference(&paren.expr),
        syn::Expr::Reference(reference) => reference.mutability.is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn method_arg_hoist_preserves_mut_reference_replay() {
        let mut stmt: syn::Stmt = syn::parse_quote! {
            (tw.lock().unwrap().w).Write(&mut (*zeroBlock.lock().unwrap())[..tw.lock().unwrap().pad]);
        };

        let hoisted = hoist_method_args_read_receiver(&mut stmt);
        let output = quote! { #(#hoisted)* #stmt }.to_string();

        assert!(
            output.contains("let __gors_premethod_arg_0 = & mut"),
            "expected mutable reference argument to be hoisted: {output}"
        );
        assert!(
            output.contains("Write (& mut * __gors_premethod_arg_0)"),
            "expected hoisted mutable reference to be replayed by reborrow: {output}"
        );
        assert!(
            !output.contains("Write (& mut __gors_premethod_arg_0)"),
            "expected no mutable borrow of the mutable-reference binding itself: {output}"
        );
    }
}
