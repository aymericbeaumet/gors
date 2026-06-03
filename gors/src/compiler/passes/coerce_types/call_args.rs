pub(super) type MutableRefCallArgs =
    std::collections::HashMap<String, std::collections::HashSet<usize>>;

#[derive(Default)]
pub(super) struct FnArgScope {
    mutable_refs: std::collections::HashSet<String>,
    generic_values: std::collections::HashSet<String>,
    has_generics: bool,
}

impl FnArgScope {
    pub(super) fn collect(sig: &syn::Signature) -> Self {
        let generic_names: std::collections::HashSet<String> = sig
            .generics
            .params
            .iter()
            .filter_map(|param| {
                if let syn::GenericParam::Type(type_param) = param {
                    Some(type_param.ident.to_string())
                } else {
                    None
                }
            })
            .collect();
        let mut mutable_refs = std::collections::HashSet::new();
        let mut generic_values = std::collections::HashSet::new();

        for input in &sig.inputs {
            let syn::FnArg::Typed(pat_type) = input else {
                continue;
            };
            let Some(name) = pat_ident_name(&pat_type.pat) else {
                continue;
            };
            if matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
            {
                mutable_refs.insert(name.clone());
            }
            if type_is_generic_param(&pat_type.ty, &generic_names)
                || type_is_cloneable_box(&pat_type.ty)
            {
                generic_values.insert(name);
            }
        }

        Self {
            mutable_refs,
            generic_values,
            has_generics: !generic_names.is_empty(),
        }
    }
}

pub(super) fn collect_mutable_ref_call_args(file: &syn::File) -> MutableRefCallArgs {
    let mut calls = std::collections::HashMap::new();
    for item in &file.items {
        match item {
            syn::Item::Fn(item_fn) => {
                let refs = mutable_ref_arg_indices(&item_fn.sig);
                if !refs.is_empty() {
                    calls.insert(item_fn.sig.ident.to_string(), refs);
                }
            }
            syn::Item::Impl(item_impl) => {
                let Some(self_ty) = super::type_path_ident_name(&item_impl.self_ty) else {
                    continue;
                };
                for item in &item_impl.items {
                    if let syn::ImplItem::Fn(func) = item {
                        let refs = mutable_ref_arg_indices(&func.sig);
                        if !refs.is_empty() {
                            calls.insert(format!("{self_ty}::{}", func.sig.ident), refs);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    calls
}

pub(super) fn coerce_scoped_call_args(
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    scope: Option<&FnArgScope>,
) {
    for arg in args {
        remove_owned_string_reference(arg);
        if matches!(arg, syn::Expr::Reference(_)) {
            continue;
        }
        if scope.is_some_and(|scope| scope.has_generics) && matches!(arg, syn::Expr::Index(_)) {
            clone_expr(arg);
            continue;
        }
        let Some(name) = super::path_ident_name(arg) else {
            continue;
        };
        if scope.is_some_and(|scope| scope.mutable_refs.contains(&name)) {
            let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
            *arg = syn::parse_quote! { &mut *#ident };
        } else if scope.is_some_and(|scope| scope.generic_values.contains(&name)) {
            clone_expr(arg);
        }
    }
}

pub(super) fn coerce_signature_call_args(
    func: &syn::Expr,
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    mutable_ref_call_args: &MutableRefCallArgs,
    pointer_cell_statics: &std::collections::HashSet<String>,
) {
    let Some(name) = call_func_name(func) else {
        return;
    };
    let Some(indices) = mutable_ref_call_args.get(&name) else {
        return;
    };
    for (index, arg) in args.iter_mut().enumerate() {
        if indices.contains(&index) {
            borrow_mut_expr(arg, pointer_cell_statics);
        }
    }
}

fn mutable_ref_arg_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
                .then_some(index)
        })
        .collect()
}

pub(super) fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    let syn::Pat::Ident(ident) = pat else {
        return None;
    };
    Some(ident.ident.to_string())
}

fn type_is_generic_param(
    ty: &syn::Type,
    generic_names: &std::collections::HashSet<String>,
) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return false;
    }
    path.path
        .segments
        .first()
        .is_some_and(|segment| generic_names.contains(&segment.ident.to_string()))
}

fn type_is_cloneable_box(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.first() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    !args
        .args
        .iter()
        .any(|arg| matches!(arg, syn::GenericArgument::Type(syn::Type::TraitObject(_))))
}

pub(super) fn call_func_name(func: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [name] => Some(name.clone()),
        [.., ty, method] => Some(format!("{ty}::{method}")),
        [] => None,
    }
}

fn remove_owned_string_reference(expr: &mut syn::Expr) {
    let syn::Expr::Reference(reference) = expr else {
        return;
    };
    if is_owned_to_string_expr(&reference.expr) {
        *expr = (*reference.expr).clone();
    }
}

fn is_owned_to_string_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::MethodCall(method) if method.method == "to_string"
    ) || matches!(expr, syn::Expr::Paren(paren) if is_owned_to_string_expr(&paren.expr))
        || matches!(expr, syn::Expr::Group(group) if is_owned_to_string_expr(&group.expr))
}

fn borrow_mut_expr(expr: &mut syn::Expr, pointer_cell_statics: &std::collections::HashSet<String>) {
    if matches!(expr, syn::Expr::Reference(_)) {
        return;
    }
    if super::is_path_ident(expr, "self") {
        return;
    }
    if super::pointer_cells::borrow_static_expr(expr, pointer_cell_statics) {
        return;
    }
    if let Some(name) = super::path_ident_name(expr) {
        let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
        *expr = syn::parse_quote! { &mut #ident };
        return;
    }
    let inner = expr.clone();
    *expr = syn::parse_quote! { &mut #inner };
}

fn clone_expr(expr: &mut syn::Expr) {
    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
}
