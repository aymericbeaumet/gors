use super::{CompiledModule, module_has_struct};

pub(super) fn replace_value_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Value") {
        return false;
    }
    module.file.items = vec![syn::parse_quote! {
        #[derive(Clone, Default)]
        pub struct Value;
    }];
    true
}
