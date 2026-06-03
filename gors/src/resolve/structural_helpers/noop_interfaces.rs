use super::{ImplSelfType, StructuralHelperFacts, has_impl, has_struct, has_trait};

pub(super) fn inject(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if facts.has_fmt_interfaces() && !has_struct(items, "__GorsNoopInterface") {
        items.insert(
            0,
            syn::parse_quote! {
                #[derive(Clone, Default)]
                struct __GorsNoopInterface;
            },
        );
    }

    if facts.has_formatter
        && facts.has_state
        && !has_impl(
            items,
            "Formatter",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl Formatter for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn Format(&mut self, _f: &mut dyn State, _verb: i32) {}
                }
            },
        );
    }

    if facts.has_stringer
        && !has_impl(
            items,
            "Stringer",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl Stringer for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn String(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if facts.has_go_stringer
        && !has_impl(
            items,
            "GoStringer",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl GoStringer for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn GoString(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if facts.has_fmt_interfaces() && !has_trait(items, "__GorsErrorExt") {
        items.insert(
            0,
            syn::parse_quote! {
                trait __GorsErrorExt {
                    fn Error(&mut self) -> String;
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for __GorsNoopInterface {
                    fn Error(&mut self) -> String { String::new() }
                }
            },
        );
    }
}
