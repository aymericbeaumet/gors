use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone)]
pub(super) struct EmbeddedInterfaceField {
    pub(super) field_ident: syn::Ident,
    pub(super) trait_path: syn::Path,
}

thread_local! {
    static BORROWED_INTERFACE_STRUCTS: RefCell<BTreeMap<String, Vec<EmbeddedInterfaceField>>> = const { RefCell::new(BTreeMap::new()) };
    static NON_CLONE_STRUCTS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub(super) fn record_borrowed_interface_struct(
    struct_name: impl Into<String>,
    fields: Vec<EmbeddedInterfaceField>,
) {
    BORROWED_INTERFACE_STRUCTS.with(|structs| {
        structs.borrow_mut().insert(struct_name.into(), fields);
    });
}

pub(super) fn clear_borrowed_interface_structs() {
    BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow_mut().clear());
}

pub(super) fn has_borrowed_interface_struct(struct_name: &str) -> bool {
    BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().contains_key(struct_name))
}

pub(super) fn borrowed_interface_fields(struct_name: &str) -> Option<Vec<EmbeddedInterfaceField>> {
    BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().get(struct_name).cloned())
}

pub(super) fn all_borrowed_interface_structs() -> BTreeMap<String, Vec<EmbeddedInterfaceField>> {
    BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().clone())
}

pub(super) fn record_struct_clone_derivability(struct_name: impl Into<String>, can_clone: bool) {
    NON_CLONE_STRUCTS.with(|structs| {
        let mut structs = structs.borrow_mut();
        let struct_name = struct_name.into();
        if can_clone {
            structs.remove(&struct_name);
        } else {
            structs.insert(struct_name);
        }
    });
}

pub(super) fn clear_struct_clone_derivability() {
    NON_CLONE_STRUCTS.with(|structs| structs.borrow_mut().clear());
}

pub(super) fn struct_can_clone(struct_name: &str) -> bool {
    NON_CLONE_STRUCTS.with(|structs| !structs.borrow().contains(struct_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_interface_struct_fields_are_recorded_and_cleared() {
        clear_borrowed_interface_structs();
        let field = EmbeddedInterfaceField {
            field_ident: syn::Ident::new("writer", proc_macro2::Span::mixed_site()),
            trait_path: syn::parse_quote! { crate::io::Writer },
        };

        record_borrowed_interface_struct("printer", vec![field]);

        assert!(has_borrowed_interface_struct("printer"));
        assert!(
            borrowed_interface_fields("printer")
                .as_ref()
                .is_some_and(|fields| fields.len() == 1)
        );
        assert_eq!(all_borrowed_interface_structs().len(), 1);

        clear_borrowed_interface_structs();

        assert!(!has_borrowed_interface_struct("printer"));
        assert!(borrowed_interface_fields("printer").is_none());
    }

    #[test]
    fn clone_derivability_defaults_to_cloneable_and_can_be_cleared() {
        clear_struct_clone_derivability();

        assert!(struct_can_clone("node"));

        record_struct_clone_derivability("node", false);
        assert!(!struct_can_clone("node"));

        record_struct_clone_derivability("node", true);
        assert!(struct_can_clone("node"));

        record_struct_clone_derivability("node", false);
        clear_struct_clone_derivability();
        assert!(struct_can_clone("node"));
    }
}
