use std::collections::HashSet;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    match import_path {
        "internal/reflectlite" => reflectlite_module(import_path, roots),
        "runtime" => runtime_module(import_path, roots),
        _ => None,
    }
}

fn runtime_module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let roots = roots?;
    if roots.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    if roots.contains("GOMAXPROCS") {
        items.push(syn::parse_quote! {
            pub fn GOMAXPROCS(mut n: isize) -> isize {
                let current = std::thread::available_parallelism()
                    .map(|parallelism| parallelism.get() as isize)
                    .unwrap_or(1)
                    .max(1);
                if n < 1 {
                    return current;
                }
                current
            }
        });
    }
    if roots.contains("GOARCH") {
        items.push(syn::parse_quote! {
            pub fn GOARCH() -> String {
                std::env::consts::ARCH.to_string()
            }
        });
    }
    if roots.contains("GOOS") {
        items.push(syn::parse_quote! {
            pub fn GOOS() -> String {
                std::env::consts::OS.to_string()
            }
        });
    }

    (!items.is_empty()).then(|| super::item_mod_for(import_path, items))
}

fn reflectlite_module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
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

    (!items.is_empty()).then(|| super::item_mod_for(import_path, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    fn tokens_for(import_path: &str, roots: &[&str]) -> Option<String> {
        let roots = roots.iter().map(|root| (*root).to_string()).collect();
        module(import_path, Some(&roots)).map(|module| module.to_token_stream().to_string())
    }

    #[test]
    fn runtime_module_emits_only_requested_roots() {
        let tokens = tokens_for("runtime", &["GOMAXPROCS", "GOOS"]).expect("runtime module");

        assert!(tokens.contains("pub fn GOMAXPROCS"), "{tokens}");
        assert!(tokens.contains("pub fn GOOS"), "{tokens}");
        assert!(!tokens.contains("pub fn GOARCH"), "{tokens}");
    }

    #[test]
    fn reflectlite_value_roots_emit_value_contract_without_swapper() {
        let tokens =
            tokens_for("internal/reflectlite", &["ValueOf", "Value::Len"]).expect("reflectlite");

        assert!(tokens.contains("pub struct Value"), "{tokens}");
        assert!(tokens.contains("pub fn Len"), "{tokens}");
        assert!(tokens.contains("pub fn Kind"), "{tokens}");
        assert!(tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_kind_root_does_not_emit_value_contract() {
        let tokens = tokens_for("internal/reflectlite", &["Slice"]).expect("reflectlite");

        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(tokens.contains("pub const Slice"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
        assert!(!tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn unknown_or_unrooted_runtime_primitives_do_not_emit_modules() {
        let empty_roots = HashSet::new();

        assert!(module("runtime", None).is_none());
        assert!(module("runtime", Some(&empty_roots)).is_none());
        assert!(module("fmt", Some(&HashSet::from(["Println".to_string()]))).is_none());
    }
}
