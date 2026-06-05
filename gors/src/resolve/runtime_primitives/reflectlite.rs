use std::collections::HashSet;

use crate::reflect_names::{
    INVALID_CONST, KIND_TYPE, REFLECTLITE_IMPORT_PATH, SLICE_CONST, SWAPPER_FUNC, VALUE_KIND_ROOT,
    VALUE_LEN_ROOT, VALUE_OF_FUNC, VALUE_TYPE,
};

pub(super) const IMPORT_PATH: &str = REFLECTLITE_IMPORT_PATH;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let roots = roots?;
    if roots.is_empty() {
        return None;
    }

    let needs_value = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            VALUE_TYPE | VALUE_OF_FUNC | SWAPPER_FUNC | VALUE_LEN_ROOT | VALUE_KIND_ROOT
        )
    });
    let needs_kind = roots
        .iter()
        .any(|root| matches!(root.as_str(), KIND_TYPE | SLICE_CONST | INVALID_CONST));
    let needs_swapper = roots.contains(SWAPPER_FUNC);

    let mut items = Vec::new();
    if needs_kind || needs_value {
        items.extend([
            syn::parse_quote! {
                pub type Kind = crate::builtin::__GorsReflectKind;
            },
            syn::parse_quote! {
                pub const Invalid: Kind = crate::builtin::__GorsReflectKind::Invalid;
            },
            syn::parse_quote! {
                pub const Slice: Kind = crate::builtin::__GorsReflectKind::Slice;
            },
        ]);
    }
    if needs_value {
        items.extend([
            syn::parse_quote! {
                pub struct Value {
                    value: Box<dyn std::any::Any>,
                }
            },
            syn::parse_quote! {
                impl Clone for Value {
                    fn clone(&self) -> Self {
                        Self {
                            value: crate::builtin::clone_any(&self.value),
                        }
                    }
                }
            },
            syn::parse_quote! {
                impl Default for Value {
                    fn default() -> Self {
                        Self {
                            value: Box::new(()) as Box<dyn std::any::Any>,
                        }
                    }
                }
            },
            syn::parse_quote! {
                impl Value {
                    pub fn Len(&self) -> isize {
                        crate::builtin::reflect_value_len(self.value.as_ref())
                    }

                    pub fn Kind(&self) -> Kind {
                        crate::builtin::reflect_value_kind(self.value.as_ref())
                    }
                }
            },
            syn::parse_quote! {
                pub fn ValueOf(i: Box<dyn std::any::Any>) -> Value {
                    Value { value: i }
                }
            },
        ]);
    }
    if needs_swapper {
        items.push(syn::parse_quote! {
            pub fn Swapper(
                slice: Box<dyn std::any::Any>,
            ) -> std::sync::Arc<
                std::sync::Mutex<
                    Option<std::sync::Arc<dyn Fn(isize, isize) -> () + Send + Sync>>
                >
            > {
                crate::builtin::reflect_value_swapper(slice.as_ref())
            }
        });
    }

    (!items.is_empty()).then(|| super::super::item_mod_for(import_path, items))
}
