use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

use proc_macro2::Span;

use super::{
    TYPE_ENV, ast, go_type_is_copy, ir, rust_safe_ident_name, synthetic_names, typeinfer,
    value_ident,
};

thread_local! {
    static SHARED_CAPTURE_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub(super) struct SharedCaptureNamesGuard {
    previous: HashSet<String>,
}

impl SharedCaptureNamesGuard {
    pub(super) fn extend(names: impl IntoIterator<Item = String>) -> Self {
        let previous = SHARED_CAPTURE_NAMES.with(|shared| {
            let previous = shared.borrow().clone();
            shared.borrow_mut().extend(names);
            previous
        });
        Self { previous }
    }
}

impl Drop for SharedCaptureNamesGuard {
    fn drop(&mut self) {
        SHARED_CAPTURE_NAMES.with(|shared| {
            *shared.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) fn is_shared_capture_name(name: &str) -> bool {
    SHARED_CAPTURE_NAMES.with(|shared| shared.borrow().contains(name))
}

pub(super) fn move_closure_shared_capture_clones(func_lit: &ast::FuncLit) -> Vec<syn::Stmt> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let names: BTreeSet<_> = ir::func_lit_captures(func_lit, &env)
            .into_iter()
            .map(|capture| capture.name)
            .collect();
        names
            .into_iter()
            .filter(|name| is_shared_capture_name(name))
            .map(|name| {
                let ident = syn::Ident::new(&rust_safe_ident_name(&name), Span::mixed_site());
                syn::parse_quote! { let #ident = #ident.clone(); }
            })
            .collect()
    })
}

pub(super) fn function_literal_shared_capture_clones(expr: &ast::Expr) -> Vec<syn::Stmt> {
    match expr {
        ast::Expr::FuncLit(func_lit) => move_closure_shared_capture_clones(func_lit),
        ast::Expr::ParenExpr(paren) => function_literal_shared_capture_clones(&paren.x),
        _ => Vec::new(),
    }
}

pub(super) fn range_function_shared_capture_clones(names: &BTreeSet<String>) -> Vec<syn::Stmt> {
    names
        .iter()
        .filter(|name| is_shared_capture_name(name))
        .map(|name| {
            let ident = value_ident(name);
            syn::parse_quote! { let #ident = #ident.clone(); }
        })
        .collect()
}

pub(super) fn shared_capture_ident(expr: &ast::Expr) -> Option<syn::Ident> {
    let ast::Expr::Ident(ident) = expr else {
        return None;
    };
    if !is_shared_capture_name(ident.name) {
        return None;
    }
    Some(value_ident(ident.name))
}

pub(super) fn shared_capture_init_expr(name: &str, init: syn::Expr) -> syn::Expr {
    if is_shared_capture_name(name) {
        syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(#init)) }
    } else {
        init
    }
}

pub(super) fn shared_capture_type(name: &str, ty: syn::Type) -> syn::Type {
    if is_shared_capture_name(name) {
        syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#ty>> }
    } else {
        ty
    }
}

pub(super) fn shared_capture_lvalue_expr(name: &str) -> Option<syn::Expr> {
    if !is_shared_capture_name(name) {
        return None;
    }
    let ident = value_ident(name);
    Some(syn::parse_quote! { (*#ident.lock().unwrap()) })
}

pub(super) fn shared_capture_read_expr(name: &str) -> Option<syn::Expr> {
    if !is_shared_capture_name(name) {
        return None;
    }
    let ident = value_ident(name);
    let go_type = TYPE_ENV.with(|env| {
        env.borrow()
            .get_var(name)
            .unwrap_or(typeinfer::GoType::Unknown)
    });
    let value_ident = synthetic_names::shared_value_ident();
    if go_type_is_copy(&go_type) {
        Some(syn::parse_quote! {{
            let #value_ident = *#ident.lock().unwrap();
            #value_ident
        }})
    } else {
        Some(syn::parse_quote! {{
            let #value_ident = #ident.lock().unwrap().clone();
            #value_ident
        }})
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn guard_restores_previous_shared_capture_names() {
        assert!(!is_shared_capture_name("outer"));
        {
            let _outer = SharedCaptureNamesGuard::extend(["outer".to_string()]);
            assert!(is_shared_capture_name("outer"));
            {
                let _inner = SharedCaptureNamesGuard::extend(["inner".to_string()]);
                assert!(is_shared_capture_name("outer"));
                assert!(is_shared_capture_name("inner"));
            }
            assert!(is_shared_capture_name("outer"));
            assert!(!is_shared_capture_name("inner"));
        }
        assert!(!is_shared_capture_name("outer"));
    }

    #[test]
    fn shared_capture_helpers_wrap_only_tracked_names() {
        let init: syn::Expr = syn::parse_quote! { value };
        let ty: syn::Type = syn::parse_quote! { String };
        let unwrapped_init = shared_capture_init_expr("x", init.clone());
        let unwrapped_ty = shared_capture_type("x", ty.clone());
        assert_eq!(
            quote!(#init).to_string(),
            quote!(#unwrapped_init).to_string()
        );
        assert_eq!(quote!(#ty).to_string(), quote!(#unwrapped_ty).to_string());

        let _shared = SharedCaptureNamesGuard::extend(["x".to_string()]);
        let wrapped_init = shared_capture_init_expr("x", init);
        let wrapped_ty = shared_capture_type("x", ty);
        let lvalue = shared_capture_lvalue_expr("x");
        let read = shared_capture_read_expr("x");
        assert!(lvalue.is_some());
        assert!(read.is_some());
        let lvalue = match lvalue {
            Some(lvalue) => lvalue,
            None => syn::parse_quote! { __missing_lvalue },
        };
        let read = match read {
            Some(read) => read,
            None => syn::parse_quote! { __missing_read },
        };

        assert!(quote!(#wrapped_init).to_string().contains("Arc :: new"));
        assert!(quote!(#wrapped_ty).to_string().contains("Arc <"));
        assert!(quote!(#lvalue).to_string().contains("lock"));
        assert!(quote!(#read).to_string().contains("__gors_shared_value"));
    }
}
