use super::{CompiledModule, module_has_struct, prune_replaced_items};
use crate::compiler::syn_inspect::type_mentions_name;
use std::collections::{BTreeMap, HashSet};

pub(super) fn replace_value_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Value") {
        return false;
    }
    let value_names = HashSet::from(["Value".to_string()]);
    prune_replaced_items(module, &value_names, &value_names);
    module
        .file
        .items
        .retain(|item| !fn_signature_mentions_type(item, &value_names));
    module.file.items.extend(value_module_file().items);
    true
}

pub(super) fn inject_missing_value_module(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved: &HashSet<String>,
) -> bool {
    if !preserved.contains("reflect")
        || modules
            .values()
            .any(|module| module.is_stdlib && module.mod_name == "reflect")
    {
        return false;
    }

    modules.insert(
        "reflect".to_string(),
        CompiledModule {
            mod_name: "reflect".to_string(),
            import_path: "reflect".to_string(),
            file: value_module_file(),
            filename: "reflect.rs".to_string(),
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        },
    );
    true
}

fn value_module_file() -> syn::File {
    syn::parse_quote! {
        #[derive(Clone, Default)]
        pub struct Value;
    }
}

fn fn_signature_mentions_type(item: &syn::Item, names: &HashSet<String>) -> bool {
    let syn::Item::Fn(func) = item else {
        return false;
    };
    func.sig.inputs.iter().any(|input| match input {
        syn::FnArg::Receiver(receiver) => type_mentions_name(&receiver.ty, names),
        syn::FnArg::Typed(pat_type) => type_mentions_name(&pat_type.ty, names),
    }) || matches!(&func.sig.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
}
