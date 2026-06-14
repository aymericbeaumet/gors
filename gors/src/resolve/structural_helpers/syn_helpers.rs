pub(super) fn has_trait(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Trait(item_trait) if item_trait.ident == name))
}

pub(super) fn trait_methods(
    items: &[syn::Item],
    trait_name: &str,
) -> Option<Vec<syn::TraitItemFn>> {
    items.iter().find_map(|item| {
        let syn::Item::Trait(item_trait) = item else {
            return None;
        };
        (item_trait.ident == trait_name).then(|| {
            item_trait
                .items
                .iter()
                .filter_map(|item| {
                    let syn::TraitItem::Fn(func) = item else {
                        return None;
                    };
                    Some(func.clone())
                })
                .collect()
        })
    })
}

pub(super) fn has_struct(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == name))
}

#[derive(Clone, Copy)]
pub(super) enum ImplSelfType<'a> {
    Named(&'a str),
    MutableReferenceToNamed(&'a str),
}

pub(super) fn has_impl(items: &[syn::Item], trait_name: &str, self_ty: ImplSelfType<'_>) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let Some((_, path, _)) = &item_impl.trait_ else {
            return false;
        };
        path.segments
            .last()
            .is_some_and(|seg| seg.ident == trait_name)
            && type_matches_impl_self(&item_impl.self_ty, self_ty)
    })
}

pub(super) fn type_matches_impl_self(ty: &syn::Type, expected: ImplSelfType<'_>) -> bool {
    match expected {
        ImplSelfType::Named(name) => type_path_matches_name(ty, name),
        ImplSelfType::MutableReferenceToNamed(name) => {
            let syn::Type::Reference(reference) = ty else {
                return false;
            };
            reference.mutability.is_some() && type_path_matches_name(&reference.elem, name)
        }
    }
}

pub(super) fn type_path_pointer_cell_inner_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    let segment = type_path.path.segments.last()?;
    if segment.ident != "GorsPtr" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut args = arguments.args.iter();
    let Some(syn::GenericArgument::Type(inner)) = args.next() else {
        return None;
    };
    if args.next().is_some() {
        return None;
    }
    type_path_ident_name(inner)
}

pub(super) fn type_is_vec_u8(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() {
        return false;
    }
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    let mut args = arguments.args.iter();
    let Some(syn::GenericArgument::Type(inner)) = args.next() else {
        return false;
    };
    args.next().is_none() && type_path_ident_name(inner).as_deref() == Some("u8")
}

pub(super) fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let segment = path.path.segments.first()?;
    if !matches!(segment.arguments, syn::PathArguments::None) {
        return None;
    }
    Some(segment.ident.to_string())
}

fn type_path_matches_name(ty: &syn::Type, name: &str) -> bool {
    type_path_ident_name(ty).is_some_and(|ident| ident == name)
}

pub(super) fn is_self_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Group(group) => is_self_expr(&group.expr),
        syn::Expr::Paren(paren) => is_self_expr(&paren.expr),
        syn::Expr::Path(path) if path.path.leading_colon.is_none() => {
            path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == "self")
        }
        _ => false,
    }
}

pub(super) fn has_method(items: &[syn::Item], ty_name: &str, method_name: &str) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let syn::Type::Path(type_path) = &*item_impl.self_ty else {
            return false;
        };
        if type_path
            .path
            .segments
            .last()
            .is_none_or(|seg| seg.ident != ty_name)
        {
            return false;
        }
        item_impl
            .items
            .iter()
            .any(|item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == method_name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_expr_matches_direct_and_wrapped_self_only() {
        let direct: syn::Expr = syn::parse_quote! { self };
        let wrapped: syn::Expr = syn::parse_quote! { ((self)) };
        let field: syn::Expr = syn::parse_quote! { self.out };
        let absolute: syn::Expr = syn::parse_quote! { ::self };

        assert!(is_self_expr(&direct));
        assert!(is_self_expr(&wrapped));
        assert!(!is_self_expr(&field));
        assert!(!is_self_expr(&absolute));
    }

    #[test]
    fn vec_u8_type_matches_single_vec_byte_type() {
        let direct: syn::Type = syn::parse_quote! { Vec<u8> };
        let qualified: syn::Type = syn::parse_quote! { std::vec::Vec<u8> };
        let wrong_inner: syn::Type = syn::parse_quote! { Vec<usize> };
        let extra_arg: syn::Type = syn::parse_quote! { Vec<u8, usize> };

        assert!(type_is_vec_u8(&direct));
        assert!(type_is_vec_u8(&qualified));
        assert!(!type_is_vec_u8(&wrong_inner));
        assert!(!type_is_vec_u8(&extra_arg));
    }
}
