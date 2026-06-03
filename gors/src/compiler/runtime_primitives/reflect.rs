use super::{CompiledModule, module_has_struct};
use std::collections::{BTreeMap, HashSet};

pub(super) fn replace_value_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Value") {
        return false;
    }
    module.file = value_module_file();
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
