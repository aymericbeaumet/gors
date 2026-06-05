use super::{CompiledModule, module_has_static, prune_replaced_items};
use crate::generated_names::{as_any_method_ident, clone_box_method_ident};
use std::collections::HashSet;

pub(super) const MODULE: &str = "os";
const FILE_TYPE: &str = "File";
const STDOUT_STATIC: &str = "Stdout";

pub(super) fn inject_stdout(module: &mut CompiledModule) -> bool {
    if !module_has_static(module, STDOUT_STATIC) {
        return false;
    }

    prune_replaced_items(
        module,
        &HashSet::from([FILE_TYPE.to_string(), STDOUT_STATIC.to_string()]),
        &HashSet::from([FILE_TYPE.to_string()]),
    );
    let as_any = as_any_method_ident();
    let clone_box = clone_box_method_ident();
    module.file.items.extend([
        syn::parse_quote! {
            #[derive(Clone, Copy, Default)]
            pub struct File;
        },
        syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            pub static Stdout: std::sync::LazyLock<crate::builtin::GorsPtr<File>> =
                std::sync::LazyLock::new(|| crate::builtin::GorsPtr::new(File));
        },
        syn::parse_quote! {
            impl File {
                fn write_stdout(b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
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

                pub fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                    File::write_stdout(b)
                }
            }
        },
        syn::parse_quote! {
            impl crate::io::Writer for File {
                fn #as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn #clone_box(&self) -> Box<dyn crate::io::Writer> {
                    Box::new(*self) as Box<dyn crate::io::Writer>
                }

                fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                    File::Write(self, b)
                }
            }
        },
        syn::parse_quote! {
            impl crate::io::Writer for crate::builtin::GorsPtr<File> {
                fn #as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn #clone_box(&self) -> Box<dyn crate::io::Writer> {
                    Box::new(self.clone()) as Box<dyn crate::io::Writer>
                }

                fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                    let _file = match self.lock() {
                        Ok(file) => file,
                        Err(err) => crate::builtin::panic_value(err),
                    };
                    File::write_stdout(b)
                }
            }
        },
    ]);
    true
}
