pub(super) fn path_starts_with(path: &syn::Path, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .zip(expected)
        .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn is_path_call_expr(func: &syn::Expr, segments: &[&str]) -> bool {
    let syn::Expr::Path(path) = func else {
        return false;
    };
    path.path.segments.len() == segments.len()
        && path
            .path
            .segments
            .iter()
            .zip(segments)
            .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    path.path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
}

pub(super) fn expr_path_ident(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    path.path.get_ident().map(ToString::to_string)
}

pub(super) fn path_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => {
            if path.qself.is_some() || path.path.segments.len() != 1 {
                return None;
            }
            path.path
                .segments
                .first()
                .map(|segment| segment.ident.to_string())
        }
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            path_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => path_ident_name(&paren.expr),
        syn::Expr::Group(group) => path_ident_name(&group.expr),
        _ => None,
    }
}

pub(super) fn is_self_expr(expr: &syn::Expr) -> bool {
    is_path_ident(expr, "self")
}

pub(super) fn is_path_ident(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == name))
}

pub(super) fn strip_paren_or_group(mut expr: &syn::Expr) -> &syn::Expr {
    loop {
        match expr {
            syn::Expr::Paren(paren) => expr = &paren.expr,
            syn::Expr::Group(group) => expr = &group.expr,
            _ => return expr,
        }
    }
}

pub(super) fn direct_clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::MethodCall(method) = expr else {
        return None;
    };
    if method.method != "clone" || !method.args.is_empty() {
        return None;
    }
    Some((*method.receiver).clone())
}

pub(super) fn clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    direct_clone_call_receiver_expr(expr).or_else(|| {
        let syn::Expr::Paren(paren) = expr else {
            return None;
        };
        clone_call_receiver_expr(&paren.expr)
    })
}

pub(super) fn stripped_clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    direct_clone_call_receiver_expr(strip_paren_or_group(expr))
}

pub(super) fn expr_path_ident_or_clone(expr: &syn::Expr) -> Option<String> {
    if let Some(receiver) = direct_clone_call_receiver_expr(expr) {
        return expr_path_ident_or_clone(&receiver);
    }
    match expr {
        syn::Expr::Group(group) => expr_path_ident_or_clone(&group.expr),
        syn::Expr::Paren(paren) => expr_path_ident_or_clone(&paren.expr),
        _ => expr_path_ident(expr),
    }
}

pub(super) fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.to_string()),
        syn::Pat::Type(pat_type) => pat_ident_name(&pat_type.pat),
        _ => None,
    }
}

pub(super) fn pat_ident_names(pat: &syn::Pat) -> Vec<String> {
    match pat {
        syn::Pat::Ident(ident) => vec![ident.ident.to_string()],
        syn::Pat::Reference(reference) => pat_ident_names(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::Type(pat_type) => pat_ident_names(&pat_type.pat),
        _ => Vec::new(),
    }
}

pub(super) fn vec_type_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    first_type_arg_if_path_last_ident(ty, "Vec").cloned()
}

pub(super) fn slice_type_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Slice(slice) = ty else {
        return None;
    };
    Some((*slice.elem).clone())
}

pub(super) fn first_type_arg_if_path_last_ident<'a>(
    ty: &'a syn::Type,
    ident: &str,
) -> Option<&'a syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != ident {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(ty)
        } else {
            None
        }
    })
}

pub(super) fn named_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => named_self_type_from_path(&path.path),
        syn::Type::Reference(reference) => named_self_type(&reference.elem),
        _ => None,
    }
}

fn named_self_type_from_path(path: &syn::Path) -> Option<String> {
    let last = path.segments.last()?;
    if matches!(last.ident.to_string().as_str(), "Arc" | "Mutex" | "GorsPtr")
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(name) = args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => named_self_type(ty),
            _ => None,
        })
    {
        return Some(name);
    }
    Some(last.ident.to_string())
}

fn direct_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => direct_self_type(&reference.elem),
        _ => None,
    }
}

pub(super) fn self_type_reachability_names(ty: &syn::Type) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = named_self_type(ty) {
        names.push(name);
    }
    if let Some(name) = direct_self_type(ty)
        && !names.contains(&name)
    {
        names.push(name);
    }
    names
}

pub(super) fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item) => Some(item.ident.to_string()),
        syn::Item::Enum(item) => Some(item.ident.to_string()),
        syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
        syn::Item::Static(item) => Some(item.ident.to_string()),
        syn::Item::Struct(item) => Some(item.ident.to_string()),
        syn::Item::Trait(item) => Some(item.ident.to_string()),
        syn::Item::Type(item) => Some(item.ident.to_string()),
        syn::Item::Union(item) => Some(item.ident.to_string()),
        syn::Item::Macro(item) => item_macro_name(item),
        _ => None,
    }
}

pub(super) fn item_macro_name(item: &syn::ItemMacro) -> Option<String> {
    item.ident
        .as_ref()
        .map(std::string::ToString::to_string)
        .or_else(|| {
            item.mac
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
        })
}

pub(super) fn macro_token_item_names(
    tokens: &proc_macro2::TokenStream,
    item_names: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    fn collect(
        tokens: proc_macro2::TokenStream,
        item_names: &std::collections::HashSet<String>,
        names: &mut std::collections::HashSet<String>,
    ) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if item_names.contains(&name) {
                        names.insert(name);
                    }
                }
                proc_macro2::TokenTree::Group(group) => {
                    collect(group.stream(), item_names, names);
                }
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }

    let mut names = std::collections::HashSet::new();
    collect(tokens.clone(), item_names, &mut names);
    names
}

pub(super) fn type_mentions_name(
    ty: &syn::Type,
    names: &std::collections::HashSet<String>,
) -> bool {
    match ty {
        syn::Type::Array(array) => type_mentions_name(&array.elem, names),
        syn::Type::Group(group) => type_mentions_name(&group.elem, names),
        syn::Type::Paren(paren) => type_mentions_name(&paren.elem, names),
        syn::Type::Path(path) => path_mentions_name(&path.path, names),
        syn::Type::Reference(reference) => type_mentions_name(&reference.elem, names),
        syn::Type::Ptr(ptr) => type_mentions_name(&ptr.elem, names),
        syn::Type::Slice(slice) => type_mentions_name(&slice.elem, names),
        syn::Type::TraitObject(trait_object) => {
            trait_object.bounds.iter().any(|bound| match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    path_mentions_name(&trait_bound.path, names)
                }
                _ => false,
            })
        }
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(|ty| type_mentions_name(ty, names)),
        _ => false,
    }
}

pub(super) fn path_mentions_name(
    path: &syn::Path,
    names: &std::collections::HashSet<String>,
) -> bool {
    path.segments.iter().any(|seg| {
        names.contains(&seg.ident.to_string())
            || match &seg.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    args.args.iter().any(|arg| match arg {
                        syn::GenericArgument::Type(ty) => type_mentions_name(ty, names),
                        syn::GenericArgument::AssocType(assoc) => {
                            type_mentions_name(&assoc.ty, names)
                        }
                        syn::GenericArgument::Constraint(constraint) => {
                            constraint.bounds.iter().any(|bound| match bound {
                                syn::TypeParamBound::Trait(trait_bound) => {
                                    path_mentions_name(&trait_bound.path, names)
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    })
                }
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(|ty| type_mentions_name(ty, names))
                        || matches!(&args.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
                }
                syn::PathArguments::None => false,
            }
    })
}

pub(super) fn trait_method_fns(
    items: &[syn::Item],
) -> std::collections::BTreeMap<String, Vec<syn::TraitItemFn>> {
    let mut traits = std::collections::BTreeMap::new();
    for item in items {
        let syn::Item::Trait(item_trait) = item else {
            continue;
        };
        let methods = item_trait
            .items
            .iter()
            .filter_map(|trait_item| match trait_item {
                syn::TraitItem::Fn(trait_fn) => Some(trait_fn.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            traits.insert(item_trait.ident.to_string(), methods);
        }
    }
    traits
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;
    use syn::parse_quote;

    #[test]
    fn is_path_call_expr_matches_exact_path_segments() {
        let expr: syn::Expr = parse_quote! { std::mem::take };
        assert!(is_path_call_expr(&expr, &["std", "mem", "take"]));
        assert!(!is_path_call_expr(&expr, &["mem", "take"]));
        assert!(!is_path_call_expr(&expr, &["std", "mem"]));

        let call: syn::Expr = parse_quote! { std::mem::take(value) };
        assert!(!is_path_call_expr(&call, &["std", "mem", "take"]));
    }

    #[test]
    fn path_ident_helpers_preserve_exact_and_stripped_semantics() {
        let ident_expr: syn::Expr = parse_quote! { value };
        let deref_expr: syn::Expr = parse_quote! { *((value)) };
        let self_expr: syn::Expr = parse_quote! { self };
        let field_expr: syn::Expr = parse_quote! { self.value };
        let qualified_expr: syn::Expr = parse_quote! { crate::value };
        let vec_ty: syn::Type = parse_quote! { Vec<u8> };

        assert_eq!(expr_path_ident(&ident_expr).as_deref(), Some("value"));
        assert_eq!(expr_path_ident(&deref_expr), None);
        assert_eq!(path_ident_name(&deref_expr).as_deref(), Some("value"));
        assert!(is_self_expr(&self_expr));
        assert!(!is_self_expr(&field_expr));
        assert!(!is_path_ident(&qualified_expr, "value"));
        assert_eq!(type_path_ident_name(&vec_ty).as_deref(), Some("Vec"));
    }

    #[test]
    fn strip_paren_or_group_keeps_non_grouped_expr() {
        let grouped: syn::Expr = parse_quote! { (((value))) };
        let stripped = strip_paren_or_group(&grouped);
        assert_eq!(expr_path_ident(stripped).as_deref(), Some("value"));

        let field: syn::Expr = parse_quote! { value.field };
        assert!(matches!(strip_paren_or_group(&field), syn::Expr::Field(_)));
    }

    #[test]
    fn clone_wrapper_helpers_preserve_wrapper_semantics() {
        fn expr_tokens(expr: syn::Expr) -> String {
            expr.to_token_stream().to_string()
        }

        let clone_call: syn::Expr = parse_quote! { value.clone() };
        let parened_clone: syn::Expr = parse_quote! { (value.clone()) };
        let grouped_clone = syn::Expr::Group(syn::ExprGroup {
            attrs: Vec::new(),
            group_token: Default::default(),
            expr: Box::new(parse_quote! { value.clone() }),
        });
        let clone_with_arg: syn::Expr = parse_quote! { value.clone(extra) };

        assert_eq!(
            direct_clone_call_receiver_expr(&clone_call).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(direct_clone_call_receiver_expr(&parened_clone).is_none());
        assert_eq!(
            clone_call_receiver_expr(&parened_clone).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(clone_call_receiver_expr(&grouped_clone).is_none());
        assert_eq!(
            stripped_clone_call_receiver_expr(&grouped_clone).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(stripped_clone_call_receiver_expr(&clone_with_arg).is_none());
    }

    #[test]
    fn expr_path_ident_or_clone_reads_path_through_clone_wrappers() {
        let plain: syn::Expr = parse_quote! { value };
        let cloned: syn::Expr = parse_quote! { ((value.clone())) };
        let grouped_clone = syn::Expr::Group(syn::ExprGroup {
            attrs: Vec::new(),
            group_token: Default::default(),
            expr: Box::new(parse_quote! { value.clone() }),
        });
        let field_clone: syn::Expr = parse_quote! { value.field.clone() };
        let clone_with_arg: syn::Expr = parse_quote! { value.clone(extra) };

        assert_eq!(expr_path_ident_or_clone(&plain).as_deref(), Some("value"));
        assert_eq!(expr_path_ident_or_clone(&cloned).as_deref(), Some("value"));
        assert_eq!(
            expr_path_ident_or_clone(&grouped_clone).as_deref(),
            Some("value")
        );
        assert_eq!(expr_path_ident_or_clone(&field_clone), None);
        assert_eq!(expr_path_ident_or_clone(&clone_with_arg), None);
    }

    #[test]
    fn pat_ident_name_reads_ident_and_typed_patterns() {
        let ident: syn::Pat = parse_quote! { value };
        assert_eq!(pat_ident_name(&ident).as_deref(), Some("value"));

        let typed: syn::Pat = syn::Pat::Type(parse_quote! { value: String });
        assert_eq!(pat_ident_name(&typed).as_deref(), Some("value"));

        let tuple: syn::Pat = parse_quote! { (left, right) };
        assert_eq!(pat_ident_name(&tuple), None);
    }

    #[test]
    fn pat_ident_names_reads_nested_bindings() {
        let pat: syn::Pat = parse_quote! { (first, &second, Some(third)) };
        assert_eq!(pat_ident_names(&pat), ["first", "second", "third"]);
    }

    #[test]
    fn type_shape_helpers_read_vec_slice_and_nested_type_args() {
        let vec_ty: syn::Type = parse_quote! { Vec<u8> };
        assert_eq!(
            vec_type_inner(&vec_ty)
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("u8")
        );

        let slice_ty: syn::Type = parse_quote! { [String] };
        assert_eq!(
            slice_type_inner(&slice_ty)
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("String")
        );

        let lazy_ty: syn::Type = parse_quote! { std::sync::LazyLock<Arc<Mutex<i32>>> };
        assert_eq!(
            first_type_arg_if_path_last_ident(&lazy_ty, "LazyLock")
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("Arc < Mutex < i32 > >")
        );
    }
}
