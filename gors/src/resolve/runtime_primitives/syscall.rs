use std::collections::HashSet;

use crate::compiler::typeinfer::{GoType, TypeEnv, TypeKind};

pub(super) const IMPORT_PATH: &str = "syscall";

pub(super) fn supplement_type_env(env: &mut TypeEnv) {
    env.set_type_kind("Errno", TypeKind::Alias(GoType::Uintptr));
    env.set_type_kind("_Socklen", TypeKind::Alias(GoType::Uint32));
    env.set_func("Errno.Error", vec![GoType::String]);
    env.set_func_params("Errno.Error", Vec::new());

    set_const(env, "ENOENT", GoType::Named("Errno".to_string()), 2);
    set_const(env, "EAGAIN", GoType::Named("Errno".to_string()), 35);
    set_const(env, "EINVAL", GoType::Named("Errno".to_string()), 22);
    set_const(env, "O_RDONLY", GoType::Int, 0);

    env.set_func("Close", vec![GoType::Error]);
    env.set_func_params("Close", vec![GoType::Int]);
    env.set_func("Open", vec![GoType::Int, GoType::Error]);
    env.set_func_params("Open", vec![GoType::String, GoType::Int, GoType::Uint32]);
    env.set_func("Read", vec![GoType::Int, GoType::Error]);
    env.set_func_params(
        "Read",
        vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))],
    );
    env.set_func("read", vec![GoType::Int, GoType::Error]);
    env.set_func_params(
        "read",
        vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))],
    );
    env.set_func("Write", vec![GoType::Int, GoType::Error]);
    env.set_func_params(
        "Write",
        vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))],
    );
    env.set_func("write", vec![GoType::Int, GoType::Error]);
    env.set_func_params(
        "write",
        vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))],
    );
    env.set_func("Seek", vec![GoType::Int64, GoType::Error]);
    env.set_func_params("Seek", vec![GoType::Int, GoType::Int64, GoType::Int]);
}

fn set_const(env: &mut TypeEnv, name: &str, ty: GoType, value: i128) {
    env.set_const_type(name, ty.clone());
    env.set_const_integer_value(name, value);
    env.set_var(name, ty);
}

pub(super) fn supplement_items(roots: Option<&HashSet<String>>, items: &mut Vec<syn::Item>) {
    let Some(roots) = roots else {
        return;
    };
    if roots.is_empty() {
        return;
    }

    let existing = item_names(items);
    let needs_errno = roots
        .iter()
        .any(|root| matches!(root.as_str(), "ENOENT" | "EAGAIN" | "EINVAL"));
    if needs_errno && !existing.contains("Errno") {
        items.extend(errno_items());
    }
    if needs_errno && !existing.contains("errors") {
        items.push(syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            static errors: std::sync::LazyLock<Vec<String>> =
                std::sync::LazyLock::new(|| Vec::new());
        });
    }

    if roots.contains("ENOENT") && !existing.contains("ENOENT") {
        items.push(syn::parse_quote! {
            pub const ENOENT: Errno = Errno(2);
        });
    }
    if roots.contains("EAGAIN") && !existing.contains("EAGAIN") {
        items.push(syn::parse_quote! {
            pub const EAGAIN: Errno = Errno(35);
        });
    }
    if roots.contains("EINVAL") && !existing.contains("EINVAL") {
        items.push(syn::parse_quote! {
            pub const EINVAL: Errno = Errno(22);
        });
    }
    if roots.contains("O_RDONLY") && !existing.contains("O_RDONLY") {
        items.push(syn::parse_quote! {
            pub const O_RDONLY: isize = 0;
        });
    }
    if needs_socklen(roots, &existing) && !existing.contains("_Socklen") {
        items.push(syn::parse_quote! {
            pub type _Socklen = u32;
        });
    }
    if roots.contains("Close") && !existing.contains("Close") {
        items.push(syn::parse_quote! {
            pub fn Close(mut fd: isize) -> Box<dyn crate::builtin::error> {
                Box::new(crate::builtin::__GorsNooperror::default())
                    as Box<dyn crate::builtin::error>
            }
        });
    }
    if roots.contains("Open") && !existing.contains("Open") {
        items.push(syn::parse_quote! {
            pub fn Open(
                mut path: String,
                mut mode: isize,
                mut perm: u32,
            ) -> (isize, Box<dyn crate::builtin::error>) {
                (
                    -1,
                    Box::new(ENOENT) as Box<dyn crate::builtin::error>,
                )
            }
        });
    }
    if needs_read(roots) && !existing.contains("read") {
        items.push(syn::parse_quote! {
            fn read(
                mut fd: isize,
                mut p: Vec<u8>,
            ) -> (isize, Box<dyn crate::builtin::error>) {
                (
                    0,
                    Box::new(crate::builtin::__GorsNooperror::default())
                        as Box<dyn crate::builtin::error>,
                )
            }
        });
    }
    if needs_write(roots) && !existing.contains("write") {
        items.push(syn::parse_quote! {
            fn write(
                mut fd: isize,
                mut p: Vec<u8>,
            ) -> (isize, Box<dyn crate::builtin::error>) {
                (
                    p.len() as isize,
                    Box::new(crate::builtin::__GorsNooperror::default())
                        as Box<dyn crate::builtin::error>,
                )
            }
        });
    }
    if roots.contains("Seek") && !existing.contains("Seek") {
        items.push(syn::parse_quote! {
            pub fn Seek(
                mut fd: isize,
                mut offset: i64,
                mut whence: isize,
            ) -> (i64, Box<dyn crate::builtin::error>) {
                (
                    0,
                    Box::new(crate::builtin::__GorsNooperror::default())
                        as Box<dyn crate::builtin::error>,
                )
            }
        });
    }
}

fn needs_socklen(roots: &HashSet<String>, existing: &HashSet<String>) -> bool {
    roots.contains("_Socklen")
        || roots.contains("Sockaddr")
        || existing.contains("Sockaddr")
        || roots
            .iter()
            .any(|root| root.ends_with("::sockaddr") || root.ends_with(".sockaddr"))
}

fn needs_read(roots: &HashSet<String>) -> bool {
    roots.contains("Read") || roots.contains("read")
}

fn needs_write(roots: &HashSet<String>) -> bool {
    roots.contains("Write") || roots.contains("write")
}

fn errno_items() -> Vec<syn::Item> {
    vec![
        syn::parse_quote! {
            #[derive(Clone, Copy, Default, Eq, Hash, PartialEq, PartialOrd)]
            pub struct Errno(pub usize);
        },
        syn::parse_quote! {
            impl Errno {
                pub fn Error(&self) -> String {
                    format!("errno {}", self.0)
                }
            }
        },
        syn::parse_quote! {
            impl std::fmt::Display for Errno {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(&self.Error())
                }
            }
        },
        syn::parse_quote! {
            impl std::error::Error for Errno {}
        },
        syn::parse_quote! {
            impl crate::builtin::error for Errno {
                fn Error(&mut self) -> String {
                    Errno::Error(self)
                }

                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn __gors_interface_key(&self) -> crate::builtin::GorsInterfaceKey {
                    crate::builtin::GorsInterfaceKey::non_comparable()
                }

                fn __gors_clone_box(&self) -> Box<dyn crate::builtin::error> {
                    Box::new(*self) as Box<dyn crate::builtin::error>
                }
            }
        },
    ]
}

fn item_names(items: &[syn::Item]) -> HashSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Const(item) => Some(item.ident.to_string()),
            syn::Item::Enum(item) => Some(item.ident.to_string()),
            syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
            syn::Item::Static(item) => Some(item.ident.to_string()),
            syn::Item::Struct(item) => Some(item.ident.to_string()),
            syn::Item::Trait(item) => Some(item.ident.to_string()),
            syn::Item::Type(item) => Some(item.ident.to_string()),
            _ => None,
        })
        .collect()
}
