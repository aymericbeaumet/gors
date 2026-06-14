use crate::reflect_names;

pub(super) fn is_fallback_expr_path(path: &syn::Path) -> bool {
    syn_path_member(path).is_some_and(is_fallback_expr_member)
}

pub(super) fn is_value_type_path(path: &syn::Path) -> bool {
    syn_path_member(path).is_some_and(|member| member == reflect_names::VALUE_TYPE)
}

fn syn_path_member(path: &syn::Path) -> Option<&syn::Ident> {
    let mut segments = path.segments.iter();
    let first = segments.next()?;
    let member = if first.ident == "crate" {
        let module = segments.next()?;
        (module.ident == reflect_names::REFLECT_MODULE).then(|| segments.next())??
    } else if first.ident == reflect_names::REFLECT_MODULE {
        segments.next()?
    } else {
        return None;
    };
    if segments.next().is_some() {
        return None;
    }
    Some(&member.ident)
}

fn is_fallback_expr_member(member: &syn::Ident) -> bool {
    member == reflect_names::VALUE_OF_FUNC || member == reflect_names::TYPE_OF_FUNC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syn_path_member_accepts_generated_reflect_paths() {
        let path: syn::Path = syn::parse_quote! { crate::reflect::ValueOf };
        let member = syn_path_member(&path);

        assert!(member.is_some(), "expected reflect member");
        let Some(member) = member else {
            return;
        };
        assert_eq!(member, reflect_names::VALUE_OF_FUNC);
    }

    #[test]
    fn syn_path_member_accepts_unqualified_reflect_paths() {
        let path: syn::Path = syn::parse_quote! { reflect::Value };
        let member = syn_path_member(&path);

        assert!(member.is_some(), "expected reflect member");
        let Some(member) = member else {
            return;
        };
        assert_eq!(member, reflect_names::VALUE_TYPE);
    }

    #[test]
    fn syn_path_member_rejects_unrelated_paths() {
        let path: syn::Path = syn::parse_quote! { other::reflect::Value };

        assert!(syn_path_member(&path).is_none());
    }

    #[test]
    fn syn_path_member_rejects_nested_reflect_member_paths() {
        let path: syn::Path = syn::parse_quote! { crate::reflect::Value::Nested };

        assert!(syn_path_member(&path).is_none());
    }

    #[test]
    fn fallback_expr_paths_accept_valueof_and_typeof() {
        let value_of: syn::Path = syn::parse_quote! { crate::reflect::ValueOf };
        let type_of: syn::Path = syn::parse_quote! { reflect::TypeOf };

        assert!(is_fallback_expr_path(&value_of));
        assert!(is_fallback_expr_path(&type_of));
    }

    #[test]
    fn fallback_expr_paths_reject_non_fallback_members() {
        let value_type: syn::Path = syn::parse_quote! { crate::reflect::Value };
        let kind_const: syn::Path = syn::parse_quote! { reflect::Slice };

        assert!(!is_fallback_expr_path(&value_type));
        assert!(!is_fallback_expr_path(&kind_const));
    }

    #[test]
    fn value_type_paths_accept_only_reflect_value_type() {
        let value_type: syn::Path = syn::parse_quote! { crate::reflect::Value };
        let value_of: syn::Path = syn::parse_quote! { reflect::ValueOf };

        assert!(is_value_type_path(&value_type));
        assert!(!is_value_type_path(&value_of));
    }
}
