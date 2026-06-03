use super::{CompiledModule, item_name, module_has_static, type_mentions_name};
use std::collections::HashSet;

pub(super) fn inject_stdout(module: &mut CompiledModule) -> bool {
    if !module_has_static(module, "Stdout") {
        return false;
    }

    let file_names = HashSet::from(["File".to_string()]);
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !type_mentions_name(&item_impl.self_ty, &file_names),
        _ => item_name(item)
            .as_deref()
            .is_none_or(|name| !matches!(name, "File" | "Stdout")),
    });
    module.file.items.extend([
        syn::parse_quote! {
            #[derive(Clone, Copy, Default)]
            pub struct File;
        },
        syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            pub static Stdout: std::sync::LazyLock<File> =
                std::sync::LazyLock::new(|| File);
        },
        syn::parse_quote! {
            impl crate::io::Writer for File {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn __gors_clone_box(&self) -> Box<dyn crate::io::Writer> {
                    Box::new(*self) as Box<dyn crate::io::Writer>
                }

                fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                    let mut stdout = std::io::stdout();
                    match std::io::Write::write_all(&mut stdout, &b) {
                        Ok(()) => (
                            b.len() as isize,
                            Box::new(crate::builtin::__GorsNooperror::default())
                                as Box<dyn crate::builtin::error>,
                        ),
                        Err(err) => (
                            0,
                            Box::new(crate::builtin::__GorsStringError(err.to_string()))
                                as Box<dyn crate::builtin::error>,
                        ),
                    }
                }
            }
        },
    ]);
    true
}
