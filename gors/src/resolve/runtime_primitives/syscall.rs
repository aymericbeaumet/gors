use std::collections::HashSet;

use crate::compiler::typeinfer::{GoType, TypeEnv, TypeKind};

pub(super) const IMPORT_PATH: &str = "syscall";

const ENVIRONMENT_ROOTS: &[&str] = &[
    "Getenv",
    "Setenv",
    "Unsetenv",
    "Clearenv",
    "Environ",
    "runtime_envs",
];

const HOST_FILESYSTEM_ROOTS: &[&str] = &["Lstat", "Fstat", "Unlink", "Rmdir"];

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
    env.set_func("Lstat", vec![GoType::Error]);
    env.set_func_params(
        "Lstat",
        vec![
            GoType::String,
            GoType::Pointer(Box::new(GoType::Named("Stat_t".to_string()))),
        ],
    );
    env.set_func("Fstat", vec![GoType::Error]);
    env.set_func_params(
        "Fstat",
        vec![
            GoType::Int,
            GoType::Pointer(Box::new(GoType::Named("Stat_t".to_string()))),
        ],
    );
    env.set_func("Unlink", vec![GoType::Error]);
    env.set_func_params("Unlink", vec![GoType::String]);
    env.set_func("Rmdir", vec![GoType::Error]);
    env.set_func_params("Rmdir", vec![GoType::String]);
    env.set_func("Getuid", vec![GoType::Int]);
    env.set_func_params("Getuid", Vec::new());
    env.set_func("Getenv", vec![GoType::String, GoType::Bool]);
    env.set_func_params("Getenv", vec![GoType::String]);
    env.set_func(
        "runtime_envs",
        vec![GoType::Slice(Box::new(GoType::String))],
    );
    env.set_func_params("runtime_envs", Vec::new());
}

fn set_const(env: &mut TypeEnv, name: &str, ty: GoType, value: i128) {
    env.set_const_type(name, ty.clone());
    env.set_const_integer_value(name, value);
    env.set_var(name, ty);
}

pub(super) fn supplement_items(roots: Option<&HashSet<String>>, items: &mut Vec<syn::Item>) {
    let needs_runtime_envs = roots
        .is_some_and(|roots| ENVIRONMENT_ROOTS.iter().any(|root| roots.contains(*root)))
        || items
            .iter()
            .any(|item| item_is_function_named(item, "runtime_envs"));
    if needs_runtime_envs {
        // `runtime_envs` is a private, bodyless runtime ABI declaration in the
        // Go syscall package. Keep the public environment algorithms generic
        // and provide only their host-owned environment snapshot boundary.
        items.retain(|item| !item_is_function_named(item, "runtime_envs"));
        items.push(runtime_envs_item());
    }

    let Some(roots) = roots else {
        return;
    };
    if roots.is_empty() {
        return;
    }

    // These are raw host-resource boundaries. Replace only an exactly rooted
    // syscall surface, leaving public Go filesystem algorithms to consume the
    // typed errors and choose their own behavior.
    items.retain(|item| {
        let syn::Item::Fn(function) = item else {
            return true;
        };
        let name = function.sig.ident.to_string();
        !HOST_FILESYSTEM_ROOTS.contains(&name.as_str()) || !roots.contains(&name)
    });

    let existing = item_names(items);
    let needs_errno = roots.iter().any(|root| {
        matches!(root.as_str(), "ENOENT" | "EAGAIN" | "EINVAL")
            || HOST_FILESYSTEM_ROOTS.contains(&root.as_str())
    });
    if needs_errno && !existing.contains("Errno") {
        items.extend(errno_items());
    }
    if needs_errno && !existing.contains("errors") {
        items.push(syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            static errors: std::sync::LazyLock<crate::builtin::GorsSliceStorage<String>> =
                std::sync::LazyLock::new(|| crate::builtin::GorsSliceStorage::default());
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
                mut p: crate::builtin::GorsSliceStorage<u8>,
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
                mut p: crate::builtin::GorsSliceStorage<u8>,
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
    if roots.contains("Lstat") {
        let lstat = lstat_item(items);
        items.push(lstat);
    }
    if roots.contains("Fstat") {
        let fstat = fstat_items(items);
        items.extend(fstat);
    }
    if roots.contains("Unlink") {
        items.push(unlink_item());
    }
    if roots.contains("Rmdir") {
        items.push(rmdir_item());
    }
    if roots.contains("Getuid") && !existing.contains("Getuid") {
        items.push(syn::parse_quote! {
            #[cfg(unix)]
            #[allow(unsafe_code)]
            pub fn Getuid() -> isize {
                unsafe extern "C" {
                    fn getuid() -> u32;
                }
                unsafe { getuid() as isize }
            }
        });
        items.push(syn::parse_quote! {
            #[cfg(not(unix))]
            pub fn Getuid() -> isize {
                -1
            }
        });
    }
}

fn runtime_envs_item() -> syn::Item {
    syn::parse_quote! {
        fn runtime_envs() -> crate::builtin::GorsSliceStorage<String> {
            std::env::vars_os()
                .map(|(__gors_key, __gors_value)| {
                    let mut __gors_entry = __gors_key.as_encoded_bytes().to_vec();
                    __gors_entry.push(b'=');
                    __gors_entry.extend_from_slice(__gors_value.as_encoded_bytes());
                    crate::builtin::go_string_from_bytes(&__gors_entry)
                })
                .collect::<crate::builtin::GorsSliceStorage<String>>()
        }
    }
}

fn item_is_function_named(item: &syn::Item, name: &str) -> bool {
    matches!(item, syn::Item::Fn(function) if function.sig.ident == name)
}

fn host_path_expr(path: &syn::Ident) -> syn::Expr {
    syn::parse_quote! {{
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            std::path::PathBuf::from(std::ffi::OsString::from_vec(
                crate::builtin::go_string_bytes(&#path),
            ))
        }
        #[cfg(not(unix))]
        {
            std::path::PathBuf::from(#path)
        }
    }}
}

fn boxed_io_errno_expr(error: &syn::Ident) -> syn::Expr {
    syn::parse_quote! {{
        let __gors_errno = match std::io::Error::raw_os_error(&#error) {
            Some(errno) if errno != 0 => i32::unsigned_abs(errno) as usize,
            _ => usize::MAX,
        };
        Box::new(Errno(__gors_errno)) as Box<dyn crate::builtin::error>
    }}
}

fn nil_error_expr() -> syn::Expr {
    syn::parse_quote! {
        Box::new(crate::builtin::__GorsNooperror::default())
            as Box<dyn crate::builtin::error>
    }
}

fn unlink_item() -> syn::Item {
    let path = syn::Ident::new("path", proc_macro2::Span::mixed_site());
    let error = syn::Ident::new("error", proc_macro2::Span::mixed_site());
    let host_path = host_path_expr(&path);
    let boxed_error = boxed_io_errno_expr(&error);
    let nil_error = nil_error_expr();

    syn::parse_quote! {
        pub fn Unlink(path: String) -> Box<dyn crate::builtin::error> {
            let __gors_path = #host_path;
            match std::fs::remove_file(__gors_path) {
                Ok(()) => #nil_error,
                Err(error) => #boxed_error,
            }
        }
    }
}

fn rmdir_item() -> syn::Item {
    let path = syn::Ident::new("path", proc_macro2::Span::mixed_site());
    let error = syn::Ident::new("error", proc_macro2::Span::mixed_site());
    let host_path = host_path_expr(&path);
    let boxed_error = boxed_io_errno_expr(&error);
    let nil_error = nil_error_expr();

    syn::parse_quote! {
        pub fn Rmdir(path: String) -> Box<dyn crate::builtin::error> {
            let __gors_path = #host_path;
            match std::fs::remove_dir(__gors_path) {
                Ok(()) => #nil_error,
                Err(error) => #boxed_error,
            }
        }
    }
}

fn stat_metadata_assignments(items: &[syn::Item]) -> (Vec<syn::Stmt>, Option<syn::Stmt>) {
    let stat_fields = struct_field_names(items, "Stat_t");
    let scalar_fields = [
        ("Dev", "dev"),
        ("Ino", "ino"),
        ("Mode", "mode"),
        ("Nlink", "nlink"),
        ("Uid", "uid"),
        ("Gid", "gid"),
        ("Rdev", "rdev"),
        ("Size", "size"),
        ("Blksize", "blksize"),
        ("Blocks", "blocks"),
    ];
    let scalar_assignments = scalar_fields
        .into_iter()
        .filter(|(field, _)| stat_fields.contains(*field))
        .map(|(field, method)| {
            let field = syn::Ident::new(field, proc_macro2::Span::mixed_site());
            let method = syn::Ident::new(method, proc_macro2::Span::mixed_site());
            syn::parse_quote! {
                __gors_stat.#field =
                    std::os::unix::fs::MetadataExt::#method(&__gors_metadata) as _;
            }
        })
        .collect::<Vec<syn::Stmt>>();
    let modified_time_assignment = ["Mtimespec", "Mtim"]
        .into_iter()
        .find(|field| stat_fields.contains(*field))
        .map(|field| {
            let field = syn::Ident::new(field, proc_macro2::Span::mixed_site());
            syn::parse_quote! {
                {
                    __gors_stat.#field.Sec =
                        std::os::unix::fs::MetadataExt::mtime(&__gors_metadata) as _;
                    __gors_stat.#field.Nsec =
                        std::os::unix::fs::MetadataExt::mtime_nsec(&__gors_metadata) as _;
                }
            }
        });

    (scalar_assignments, modified_time_assignment)
}

fn lstat_item(items: &[syn::Item]) -> syn::Item {
    let (scalar_assignments, modified_time_assignment) = stat_metadata_assignments(items);
    let path = syn::Ident::new("path", proc_macro2::Span::mixed_site());
    let metadata_error = syn::Ident::new("error", proc_macro2::Span::mixed_site());
    let host_path = host_path_expr(&path);
    let boxed_metadata_error = boxed_io_errno_expr(&metadata_error);
    let nil_error = nil_error_expr();

    syn::parse_quote! {
        pub fn Lstat(
            path: String,
            stat: crate::builtin::GorsPtr<Stat_t>,
        ) -> Box<dyn crate::builtin::error> {
            let __gors_path = #host_path;
            let __gors_metadata = match std::fs::symlink_metadata(__gors_path) {
                Ok(metadata) => metadata,
                Err(error) => return #boxed_metadata_error,
            };
            let mut __gors_stat = match stat.lock() {
                Ok(stat) => stat,
                Err(_) => {
                    return Box::new(Errno(usize::MAX))
                        as Box<dyn crate::builtin::error>;
                }
            };
            *__gors_stat = Default::default();
            #[cfg(unix)]
            {
                #(#scalar_assignments)*
                #modified_time_assignment
            }
            #nil_error
        }
    }
}

fn fstat_items(items: &[syn::Item]) -> Vec<syn::Item> {
    let (scalar_assignments, modified_time_assignment) = stat_metadata_assignments(items);
    let clone_error = syn::Ident::new("error", proc_macro2::Span::mixed_site());
    let metadata_error = syn::Ident::new("error", proc_macro2::Span::mixed_site());
    let boxed_clone_error = boxed_io_errno_expr(&clone_error);
    let boxed_metadata_error = boxed_io_errno_expr(&metadata_error);
    let nil_error = nil_error_expr();

    vec![
        syn::parse_quote! {
            #[cfg(unix)]
            #[allow(unsafe_code)]
            pub fn Fstat(
                fd: isize,
                stat: crate::builtin::GorsPtr<Stat_t>,
            ) -> Box<dyn crate::builtin::error> {
                let __gors_raw_fd = match i32::try_from(fd) {
                    Ok(raw_fd) if raw_fd >= 0 => raw_fd,
                    _ => {
                        return Box::new(Errno(usize::MAX))
                            as Box<dyn crate::builtin::error>;
                    }
                };
                let __gors_borrowed_fd = unsafe {
                    std::os::fd::BorrowedFd::borrow_raw(__gors_raw_fd)
                };
                let __gors_owned_fd = match std::os::fd::BorrowedFd::try_clone_to_owned(
                    &__gors_borrowed_fd,
                ) {
                    Ok(owned_fd) => owned_fd,
                    Err(error) => return #boxed_clone_error,
                };
                let __gors_file = std::fs::File::from(__gors_owned_fd);
                let __gors_metadata = match std::fs::File::metadata(&__gors_file) {
                    Ok(metadata) => metadata,
                    Err(error) => return #boxed_metadata_error,
                };
                let mut __gors_stat = match stat.lock() {
                    Ok(stat) => stat,
                    Err(_) => {
                        return Box::new(Errno(usize::MAX))
                            as Box<dyn crate::builtin::error>;
                    }
                };
                *__gors_stat = Default::default();
                #(#scalar_assignments)*
                #modified_time_assignment
                #nil_error
            }
        },
        syn::parse_quote! {
            #[cfg(not(unix))]
            pub fn Fstat(
                _fd: isize,
                _stat: crate::builtin::GorsPtr<Stat_t>,
            ) -> Box<dyn crate::builtin::error> {
                Box::new(Errno(usize::MAX)) as Box<dyn crate::builtin::error>
            }
        },
    ]
}

fn struct_field_names(items: &[syn::Item], struct_name: &str) -> HashSet<String> {
    items
        .iter()
        .find_map(|item| {
            let syn::Item::Struct(item_struct) = item else {
                return None;
            };
            (item_struct.ident == struct_name).then(|| {
                item_struct
                    .fields
                    .iter()
                    .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
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
            #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, PartialOrd)]
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
                fn Error(&self) -> String {
                    Errno::Error(self)
                }

                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn __gors_interface_key(&self) -> crate::builtin::GorsInterfaceKey {
                    crate::builtin::GorsInterfaceKey::for_comparable(self)
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
