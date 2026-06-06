use super::{CompiledModule, module_has_struct, prune_replaced_items};
use crate::compiler::syn_inspect::type_mentions_name;
use crate::reflect_names;
use std::collections::{BTreeMap, HashSet};

pub(super) const MODULE: &str = reflect_names::REFLECT_MODULE;

pub(super) fn replace_value_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, reflect_names::VALUE_TYPE) {
        return false;
    }
    let replaced_names = HashSet::from([
        reflect_names::VALUE_TYPE.to_string(),
        reflect_names::MAP_ITER_TYPE.to_string(),
    ]);
    let signature_names = HashSet::from([reflect_names::VALUE_TYPE.to_string()]);
    prune_replaced_items(module, &replaced_names, &replaced_names);
    prune_impl_items_with_signature_mentions_type(module, &signature_names);
    module
        .file
        .items
        .retain(|item| !fn_signature_mentions_type(item, &signature_names));
    module.file.items.extend(value_module_file().items);
    module.file.items.extend(value_method_items(module));
    module.file.items.extend(map_iter_module_file().items);
    true
}

pub(super) fn inject_missing_value_module(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved: &HashSet<String>,
) -> bool {
    if !preserved.contains(MODULE)
        || modules
            .values()
            .any(|module| module.is_stdlib && module.mod_name == MODULE)
    {
        return false;
    }

    modules.insert(
        MODULE.to_string(),
        CompiledModule {
            mod_name: MODULE.to_string(),
            import_path: MODULE.to_string(),
            file: value_module_file(),
            filename: format!("{MODULE}.rs"),
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        },
    );
    true
}

fn value_module_file() -> syn::File {
    syn::parse_quote! {
        #[derive(Clone, Default, PartialEq)]
        pub struct Value;
    }
}

fn map_iter_module_file() -> syn::File {
    syn::parse_quote! {
        #[derive(Clone, Default)]
        pub struct MapIter;

        impl MapIter {
            pub fn Key(mut iter: crate::builtin::GorsPtr<Self>) -> Value {
                let _ = iter;
                Value::default()
            }

            pub fn Next(mut iter: crate::builtin::GorsPtr<Self>) -> bool {
                let _ = iter;
                false
            }

            pub fn Value(mut iter: crate::builtin::GorsPtr<Self>) -> Value {
                let _ = iter;
                Value::default()
            }
        }
    }
}

fn value_method_items(module: &CompiledModule) -> Vec<syn::Item> {
    if !module_has_trait(module, reflect_names::TYPE_TYPE)
        || !module_has_struct(module, &format!("__GorsNoop{}", reflect_names::TYPE_TYPE))
    {
        return Vec::new();
    }
    let file: syn::File = syn::parse_quote! {
        impl Value {
            pub fn Bool(&self) -> bool {
                false
            }

            pub fn Bytes(&self) -> Vec<u8> {
                Vec::new()
            }

            pub fn CanAddr(&self) -> bool {
                false
            }

            pub fn CanInterface(&self) -> bool {
                false
            }

            pub fn Complex(&self) -> crate::builtin::Complex128 {
                Default::default()
            }

            pub fn Elem(&self) -> Value {
                Value::default()
            }

            pub fn Field(&self, _i: isize) -> Value {
                Value::default()
            }

            pub fn Float(&self) -> f64 {
                0.0
            }

            pub fn Index(&self, _i: isize) -> Value {
                Value::default()
            }

            pub fn Int(&self) -> i64 {
                0
            }

            pub fn Interface(&self) -> Box<dyn std::any::Any> {
                Box::new(())
            }

            pub fn IsNil(&self) -> bool {
                true
            }

            pub fn IsValid(&self) -> bool {
                false
            }

            pub fn Kind(&self) -> Kind {
                Default::default()
            }

            pub fn Len(&self) -> isize {
                0
            }

            pub fn MapRange(&self) -> crate::builtin::GorsPtr<MapIter> {
                crate::builtin::GorsPtr::new(MapIter::default())
            }

            pub fn NumField(&self) -> isize {
                0
            }

            pub fn Pointer(&self) -> usize {
                0
            }

            pub fn String(&self) -> String {
                String::new()
            }

            pub fn Type(&self) -> Box<dyn Type> {
                Box::new(__GorsNoopType::default()) as Box<dyn Type>
            }

            pub fn Uint(&self) -> u64 {
                0
            }

            pub fn UnsafePointer(&self) -> usize {
                0
            }

            pub fn pointer(&self) -> usize {
                0
            }
        }

        impl PartialEq for Box<dyn Type> {
            fn eq(&self, _other: &Self) -> bool {
                false
            }
        }
    };
    file.items
}

fn module_has_trait(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Trait(item_trait) if item_trait.ident == name))
}

fn fn_signature_mentions_type(item: &syn::Item, names: &HashSet<String>) -> bool {
    let syn::Item::Fn(func) = item else {
        return false;
    };
    signature_mentions_type(&func.sig, names)
}

fn prune_impl_items_with_signature_mentions_type(
    module: &mut CompiledModule,
    names: &HashSet<String>,
) {
    for item in &mut module.file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        item_impl.items.retain(|impl_item| match impl_item {
            syn::ImplItem::Fn(func) => !signature_mentions_type(&func.sig, names),
            _ => true,
        });
    }
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !item_impl.items.is_empty(),
        _ => true,
    });
}

fn signature_mentions_type(sig: &syn::Signature, names: &HashSet<String>) -> bool {
    sig.inputs.iter().any(|input| match input {
        syn::FnArg::Receiver(receiver) => type_mentions_name(&receiver.ty, names),
        syn::FnArg::Typed(pat_type) => type_mentions_name(&pat_type.ty, names),
    }) || matches!(&sig.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
}
