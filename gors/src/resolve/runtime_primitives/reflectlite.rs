use std::collections::HashSet;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let roots = roots?;
    if roots.is_empty() {
        return None;
    }

    let needs_value = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Value" | "ValueOf" | "Swapper" | "Value::Len" | "Value::Kind"
        )
    });
    let needs_kind = roots
        .iter()
        .any(|root| matches!(root.as_str(), "Kind" | "Slice" | "Invalid"));
    let needs_swapper = roots.contains("Swapper");

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
