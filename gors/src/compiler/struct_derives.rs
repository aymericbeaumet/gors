use super::{ast, typeinfer};

pub(super) struct State {
    pub(super) needs_manual_default: bool,
    pub(super) needs_manual_clone: bool,
    pub(super) cannot_manual_clone: bool,
    pub(super) cannot_derive_clone: bool,
    pub(super) cannot_derive_partial_eq: bool,
    pub(super) cannot_default: bool,
    pub(super) can_derive_copy: bool,
    pub(super) needs_manual_send_sync: bool,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            needs_manual_default: false,
            needs_manual_clone: false,
            cannot_manual_clone: false,
            cannot_derive_clone: false,
            cannot_derive_partial_eq: false,
            cannot_default: false,
            can_derive_copy: true,
            needs_manual_send_sync: false,
        }
    }

    pub(super) fn record_field(&mut self, facts: FieldFacts) {
        self.needs_manual_default |= facts.needs_manual_default;
        self.needs_manual_clone |= facts.cannot_derive_clone && facts.contains_any;
        self.cannot_manual_clone |= facts.cannot_derive_clone && !facts.contains_any;
        self.cannot_derive_clone |= facts.cannot_derive_clone;
        self.cannot_derive_partial_eq |= facts.cannot_derive_partial_eq;
        self.cannot_default |= facts.cannot_default;
        self.can_derive_copy &= facts.can_derive_copy;
        self.needs_manual_send_sync |= facts.contains_any;
    }
}

#[derive(Clone, Copy)]
pub(super) struct FieldFacts {
    needs_manual_default: bool,
    cannot_derive_clone: bool,
    cannot_derive_partial_eq: bool,
    cannot_default: bool,
    can_derive_copy: bool,
    contains_any: bool,
}

impl FieldFacts {
    pub(super) fn collect(
        field_type: &ast::Expr,
        struct_ident: &syn::Ident,
        field_go_type: &typeinfer::GoType,
        field_is_error: bool,
        has_interface_trait_path: bool,
        has_borrowed_interface_trait_path: bool,
    ) -> Self {
        let contains_func = super::contains_func_type(field_type);
        let contains_any = super::contains_any_type(field_type);
        let needs_manual_default = super::contains_array_type(field_type)
            || contains_func
            || field_is_error
            || has_borrowed_interface_trait_path;
        let cannot_derive_clone = contains_any
            || (!field_is_error && super::contains_interface_type(field_type))
            || (!field_is_error && has_interface_trait_path);
        let cannot_derive_partial_eq =
            field_is_error || !super::expr_supports_derived_partial_eq(field_type, struct_ident);
        let cannot_default = has_borrowed_interface_trait_path;
        let can_derive_copy = !contains_func
            && !has_interface_trait_path
            && (super::self_referential_pointer_type(field_type, struct_ident).is_some()
                || super::go_type_is_copy(field_go_type));

        Self {
            needs_manual_default,
            cannot_derive_clone,
            cannot_derive_partial_eq,
            cannot_default,
            can_derive_copy,
            contains_any,
        }
    }
}

pub(super) fn manual_clone_expr_for_field(
    field_ident: &syn::Ident,
    field_go_type: &typeinfer::GoType,
) -> syn::Expr {
    match super::resolved_go_type(field_go_type) {
        typeinfer::GoType::Any => {
            syn::parse_quote! { crate::builtin::clone_any(&self.#field_ident) }
        }
        typeinfer::GoType::Slice(inner) if matches!(*inner, typeinfer::GoType::Any) => {
            syn::parse_quote! {
                self.#field_ident
                    .iter()
                    .map(|__gors_any_item| crate::builtin::clone_any(__gors_any_item))
                    .collect()
            }
        }
        _ => syn::parse_quote! { self.#field_ident.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    #[test]
    fn manual_clone_for_any_fields_uses_runtime_clone_any() {
        let field_ident = syn::Ident::new("value", proc_macro2::Span::mixed_site());

        let clone_expr = manual_clone_expr_for_field(&field_ident, &typeinfer::GoType::Any);
        let tokens = clone_expr.to_token_stream().to_string();

        assert!(tokens.contains("clone_any"), "{tokens}");
        assert!(!tokens.contains("self . value . clone"), "{tokens}");
    }

    #[test]
    fn manual_clone_for_slice_any_clones_each_element_through_runtime() {
        let field_ident = syn::Ident::new("values", proc_macro2::Span::mixed_site());
        let field_type = typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Any));

        let clone_expr = manual_clone_expr_for_field(&field_ident, &field_type);
        let tokens = clone_expr.to_token_stream().to_string();

        assert!(tokens.contains("clone_any"), "{tokens}");
        assert!(tokens.contains("collect"), "{tokens}");
    }

    #[test]
    fn struct_state_records_any_fields_as_manual_clone_send_sync() {
        let mut state = State::new();
        state.record_field(FieldFacts {
            needs_manual_default: false,
            cannot_derive_clone: true,
            cannot_derive_partial_eq: true,
            cannot_default: false,
            can_derive_copy: false,
            contains_any: true,
        });

        assert!(state.needs_manual_clone);
        assert!(state.needs_manual_send_sync);
        assert!(!state.cannot_manual_clone);
        assert!(!state.can_derive_copy);
    }
}
