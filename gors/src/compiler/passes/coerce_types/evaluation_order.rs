use syn::visit_mut::{self, VisitMut};

use super::super::super::syn_inspect::{
    expr_contains_path_ident, mut_borrowed_path_name, receiver_root_ident_name,
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
            let temp = quote::format_ident!("__gors_preborrow_arg_{}", hoisted.len());
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
        let temp = quote::format_ident!("__gors_premethod_arg_{}", hoisted.len());
        let value = arg.clone();
        *arg = syn::parse_quote! { #temp };
        hoisted.push(syn::parse_quote! {
            let #temp = #value;
        });
    }
    hoisted
}
