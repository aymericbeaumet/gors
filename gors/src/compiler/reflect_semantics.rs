pub(super) const MODULE: &str = "reflect";
pub(super) const KIND_METHOD: &str = "Kind";
pub(super) const TYPE_OF_FUNC: &str = "TypeOf";
pub(super) const VALUE_OF_FUNC: &str = "ValueOf";
pub(super) const VALUE_TYPE: &str = "Value";

pub(super) fn syn_path_member(path: &syn::Path) -> Option<&syn::Ident> {
    let mut segments = path.segments.iter();
    let first = segments.next()?;
    let member = if first.ident == "crate" {
        let module = segments.next()?;
        (module.ident == MODULE).then(|| segments.next())??
    } else if first.ident == MODULE {
        segments.next()?
    } else {
        return None;
    };
    if segments.next().is_some() {
        return None;
    }
    Some(&member.ident)
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
        assert_eq!(member, VALUE_OF_FUNC);
    }

    #[test]
    fn syn_path_member_accepts_unqualified_reflect_paths() {
        let path: syn::Path = syn::parse_quote! { reflect::Value };
        let member = syn_path_member(&path);

        assert!(member.is_some(), "expected reflect member");
        let Some(member) = member else {
            return;
        };
        assert_eq!(member, VALUE_TYPE);
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
}
