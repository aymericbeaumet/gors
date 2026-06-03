use syn::visit_mut::{self, VisitMut};

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

fn receiver_root_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(_) => super::syntax::path_ident_name(expr),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            receiver_root_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => receiver_root_ident_name(&paren.expr),
        syn::Expr::Group(group) => receiver_root_ident_name(&group.expr),
        syn::Expr::Field(field) => receiver_root_ident_name(&field.base),
        syn::Expr::Reference(reference) => receiver_root_ident_name(&reference.expr),
        syn::Expr::MethodCall(method)
            if method.args.is_empty() && is_transparent_receiver_method(&method.method) =>
        {
            receiver_root_ident_name(&method.receiver)
        }
        _ => None,
    }
}

fn is_transparent_receiver_method(method: &syn::Ident) -> bool {
    matches!(
        method.to_string().as_str(),
        "as_mut" | "as_ref" | "clone" | "lock" | "unwrap"
    )
}

fn mut_borrowed_path_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    super::syntax::path_ident_name(&reference.expr)
}

fn expr_contains_path_ident(expr: &syn::Expr, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_path(&mut self, path: &syn::ExprPath) {
            if path.path.leading_colon.is_none()
                && path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == self.name)
            {
                self.found = true;
            }
            syn::visit::visit_expr_path(self, path);
        }
    }

    let mut finder = Finder { name, found: false };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

pub(super) fn expr_contains_any_path_ident(
    expr: &syn::Expr,
    names: &std::collections::HashSet<String>,
) -> bool {
    names
        .iter()
        .any(|name| expr_contains_path_ident(expr, name))
}

pub(super) fn expr_contains_method_call(expr: &syn::Expr, method: &str) -> bool {
    struct Finder<'a> {
        method: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if call.method == self.method {
                self.found = true;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        method,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}
