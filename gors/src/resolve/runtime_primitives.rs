use std::collections::HashSet;

mod reflectlite;
mod runtime;
mod syscall;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    match import_path {
        reflectlite::IMPORT_PATH => reflectlite::module(import_path, roots),
        runtime::IMPORT_PATH => runtime::module(import_path, roots),
        _ => None,
    }
}

pub(super) fn supplement_items(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    items: &mut Vec<syn::Item>,
) {
    if import_path == syscall::IMPORT_PATH {
        syscall::supplement_items(roots, items);
    }
}

pub(super) fn supplement_type_env(
    import_path: &str,
    env: &mut crate::compiler::typeinfer::TypeEnv,
) {
    match import_path {
        reflectlite::IMPORT_PATH => reflectlite::supplement_type_env(env),
        syscall::IMPORT_PATH => syscall::supplement_type_env(env),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::{GoType, TypeEnv, TypeKind};
    use quote::ToTokens;
    use quote::quote;

    fn tokens_for(import_path: &str, roots: &[&str]) -> Option<String> {
        let roots = roots.iter().map(|root| (*root).to_string()).collect();
        module(import_path, Some(&roots)).map(|module| module.to_token_stream().to_string())
    }

    fn required_tokens_for(import_path: &str, roots: &[&str]) -> String {
        let tokens = tokens_for(import_path, roots);
        assert!(
            tokens.is_some(),
            "expected runtime primitive module for {import_path}"
        );
        tokens.unwrap_or_default()
    }

    fn supplemented_tokens_for(
        import_path: &str,
        roots: &[&str],
        mut items: Vec<syn::Item>,
    ) -> String {
        let roots = roots.iter().map(|root| (*root).to_string()).collect();
        supplement_items(import_path, Some(&roots), &mut items);
        quote! { #(#items)* }.to_string()
    }

    fn syscall_host_test_builtin() -> syn::ItemMod {
        syn::parse_quote! {
            mod builtin {
                pub struct GorsInterfaceKey;
                pub type GorsSliceStorage<T> = Vec<T>;

                impl GorsInterfaceKey {
                    pub fn for_comparable<T: ?Sized>(_value: &T) -> Self {
                        Self
                    }
                }

                pub trait error: Send + Sync {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                    fn __gors_interface_key(&self) -> GorsInterfaceKey;
                    fn __gors_clone_box(&self) -> Box<dyn error>;
                    fn Error(&self) -> String;
                }

                #[derive(Default)]
                pub struct __GorsNooperror;

                impl error for __GorsNooperror {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                        None
                    }

                    fn __gors_interface_key(&self) -> GorsInterfaceKey {
                        GorsInterfaceKey
                    }

                    fn __gors_clone_box(&self) -> Box<dyn error> {
                        Box::new(Self)
                    }

                    fn Error(&self) -> String {
                        String::new()
                    }
                }

                pub struct GorsPtr<T>(std::sync::Arc<std::sync::Mutex<T>>);

                impl<T> GorsPtr<T> {
                    pub fn new(value: T) -> Self {
                        Self(std::sync::Arc::new(std::sync::Mutex::new(value)))
                    }

                    pub fn lock(
                        &self,
                    ) -> std::sync::LockResult<std::sync::MutexGuard<'_, T>> {
                        self.0.lock()
                    }
                }

                impl<T> Clone for GorsPtr<T> {
                    fn clone(&self) -> Self {
                        Self(self.0.clone())
                    }
                }

                pub fn go_string_bytes(value: &String) -> Vec<u8> {
                    value.as_bytes().to_vec()
                }
            }
        }
    }

    fn compile_and_run_syscall_host_test(
        file: &syn::File,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let build = tempfile::tempdir()?;
        let source = build.path().join("main.rs");
        let executable = build.path().join("main");
        std::fs::write(&source, prettyplease::unparse(file))?;
        let compile = std::process::Command::new("rustup")
            .args(["run", "1.96.0", "rustc"])
            .arg(&source)
            .args(["--edition=2024", "-o"])
            .arg(&executable)
            .output()?;
        assert!(
            compile.status.success(),
            "generated syscall host boundary failed to compile:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(executable).output()?;
        assert!(
            run.status.success(),
            "generated syscall host boundary failed at runtime:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        Ok(())
    }

    #[test]
    fn runtime_module_emits_only_requested_roots() {
        let tokens = required_tokens_for("runtime", &["GOMAXPROCS", "GOOS", "GOROOT", "stringer"]);

        assert!(tokens.contains("pub fn GOMAXPROCS"), "{tokens}");
        assert!(tokens.contains("pub fn GOOS"), "{tokens}");
        assert!(tokens.contains("pub fn GOROOT"), "{tokens}");
        assert!(tokens.contains("pub trait stringer"), "{tokens}");
        assert!(!tokens.contains("pub fn GOARCH"), "{tokens}");
    }

    #[test]
    fn runtime_module_emits_stack_roots() {
        let tokens = required_tokens_for(
            "runtime",
            &["Callers", "CallersFrames", "Frames", "Frames::Next"],
        );

        assert!(tokens.contains("pub fn Callers"), "{tokens}");
        assert!(tokens.contains("pc : & mut [usize]"), "{tokens}");
        assert!(tokens.contains("pub fn CallersFrames"), "{tokens}");
        assert!(tokens.contains("GorsSliceStorage < usize >"), "{tokens}");
        assert!(tokens.contains("pub struct Frame"), "{tokens}");
        assert!(tokens.contains("pub struct Frames"), "{tokens}");
        assert!(tokens.contains("GorsSliceStorage < Frame >"), "{tokens}");
        assert!(tokens.contains("GorsSliceStorage :: default"), "{tokens}");
        assert!(!tokens.contains("frames : Vec < Frame >"), "{tokens}");
        assert!(!tokens.contains("callers : Vec < usize >"), "{tokens}");
        assert!(tokens.contains("pub fn Next"), "{tokens}");
        assert!(tokens.contains("Frame :: default"), "{tokens}");
    }

    #[test]
    fn runtime_module_emits_rooted_keep_alive_barrier() {
        let tokens = required_tokens_for("runtime", &["KeepAlive"]);

        assert!(tokens.contains("pub fn KeepAlive < T >"), "{tokens}");
        assert!(
            tokens.contains("std :: hint :: black_box (value)"),
            "{tokens}"
        );
        assert!(!tokens.contains("pub fn GOOS"), "{tokens}");
    }

    #[test]
    fn reflectlite_value_roots_emit_value_contract_without_swapper() {
        let tokens = required_tokens_for("internal/reflectlite", &["ValueOf", "Value::Len"]);

        assert!(tokens.contains("pub struct Value"), "{tokens}");
        assert!(tokens.contains("pub fn Len"), "{tokens}");
        assert!(tokens.contains("pub fn Kind"), "{tokens}");
        assert!(tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_kind_root_does_not_emit_value_contract() {
        let tokens = required_tokens_for("internal/reflectlite", &["Slice"]);

        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(tokens.contains("pub const Slice"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
        assert!(!tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_type_roots_emit_typeof_comparable_contract() {
        let tokens = required_tokens_for(
            "internal/reflectlite",
            &["TypeOf", "Type::Comparable", "rtype"],
        );

        assert!(tokens.contains("pub struct Type"), "{tokens}");
        assert!(tokens.contains("pub type rtype"), "{tokens}");
        assert!(tokens.contains("pub fn TypeOf"), "{tokens}");
        assert!(tokens.contains("pub fn Comparable"), "{tokens}");
        assert!(tokens.contains("pub fn String"), "{tokens}");
        assert!(tokens.contains("impl PartialEq for Type"), "{tokens}");
        assert!(tokens.contains("impl Eq for Type"), "{tokens}");
        assert!(tokens.contains("any_dynamic_type_id"), "{tokens}");
        assert!(!tokens.contains("as_ref () . type_id ()"), "{tokens}");
        assert!(tokens.contains("reflect_type_comparable"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_type_env_matches_concrete_host_handle_abi() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Type", TypeKind::Interface);
        supplement_type_env("internal/reflectlite", &mut env);

        assert_eq!(env.get_type_kind("Type"), Some(&TypeKind::Struct));
        assert!(
            !env.is_interface("Type"),
            "the concrete host handle must not retain source-interface dispatch facts"
        );
        assert_eq!(
            env.get_func_returns("TypeOf"),
            vec![GoType::Named("Type".to_string())]
        );
        assert_eq!(env.get_func_params("TypeOf"), vec![GoType::Any]);
        assert_eq!(env.get_func_returns("Type.Comparable"), vec![GoType::Bool]);
        assert_eq!(env.get_func_params("Type.Comparable"), Vec::<GoType>::new());
    }

    #[test]
    fn syscall_supplements_write_boundary_and_socklen_alias() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Sockaddr", "Write"],
            vec![syn::parse_quote! {
                pub trait Sockaddr {
                    fn sockaddr(
                        &mut self,
                    ) -> (usize, _Socklen, Box<dyn crate::builtin::error>);
                }
            }],
        );

        assert!(tokens.contains("pub type _Socklen = u32"), "{tokens}");
        assert!(tokens.contains("fn write"), "{tokens}");
        assert!(tokens.contains("GorsSliceStorage < u8 >"), "{tokens}");
        assert!(tokens.contains("p . len () as isize"), "{tokens}");
        assert!(!tokens.contains("fn read"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_getuid_host_boundary() {
        let tokens = supplemented_tokens_for("syscall", &["Getuid"], Vec::new());

        assert!(tokens.contains("pub fn Getuid"), "{tokens}");
        assert!(tokens.contains("unsafe extern"), "{tokens}");
        assert!(tokens.contains("fn getuid"), "{tokens}");
        assert!(!tokens.contains("fn write"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_getenv_runtime_abi_without_replacing_getenv() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Getenv"],
            vec![syn::parse_quote! {
                pub fn Getenv(key: String) -> (String, bool) {
                    generic_getenv_marker();
                    (key, true)
                }
            }],
        );

        assert!(tokens.contains("pub fn Getenv"), "{tokens}");
        assert!(tokens.contains("generic_getenv_marker"), "{tokens}");
        assert!(tokens.contains("fn runtime_envs"), "{tokens}");
        assert!(tokens.contains("GorsSliceStorage < String >"), "{tokens}");
        assert!(
            tokens.contains("collect :: < crate :: builtin :: GorsSliceStorage < String > >"),
            "{tokens}"
        );
        assert!(tokens.contains("std :: env :: vars_os"), "{tokens}");
        assert!(tokens.contains("as_encoded_bytes"), "{tokens}");
        assert!(tokens.contains("__gors_entry . push (b'=')"), "{tokens}");
        assert!(
            tokens.contains("crate :: builtin :: go_string_from_bytes"),
            "{tokens}"
        );
    }

    #[test]
    fn syscall_synthetic_go_slice_abis_use_owned_storage() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["EINVAL", "Read", "Write", "Getenv"],
            Vec::new(),
        );

        assert!(
            tokens.contains("LazyLock < crate :: builtin :: GorsSliceStorage < String > >"),
            "{tokens}"
        );
        assert!(tokens.contains("GorsSliceStorage :: default"), "{tokens}");
        assert_eq!(tokens.matches("GorsSliceStorage < u8 >").count(), 2);
        assert!(tokens.contains("GorsSliceStorage < String >"), "{tokens}");
        assert!(!tokens.contains("LazyLock < Vec < String > >"), "{tokens}");
        assert!(!tokens.contains("mut p : Vec < u8 >"), "{tokens}");
    }

    #[test]
    fn syscall_environment_family_roots_activate_runtime_envs_host_abi() {
        for root in ["Setenv", "Unsetenv", "Clearenv", "Environ"] {
            let tokens = supplemented_tokens_for("syscall", &[root], Vec::new());

            assert!(tokens.contains("fn runtime_envs"), "{root}: {tokens}");
            assert!(tokens.contains("std :: env :: vars_os"), "{root}: {tokens}");
        }
    }

    #[test]
    fn syscall_reachable_runtime_envs_item_activates_host_abi() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Getuid"],
            vec![syn::parse_quote! {
                fn runtime_envs() -> crate::builtin::GorsSliceStorage<String> {
                    stale_bodyless_fallback_marker();
                    crate::builtin::GorsSliceStorage::default()
                }
            }],
        );

        assert!(tokens.contains("std :: env :: vars_os"), "{tokens}");
        assert!(
            !tokens.contains("stale_bodyless_fallback_marker"),
            "{tokens}"
        );
    }

    #[test]
    fn syscall_unfiltered_runtime_envs_item_activates_host_abi() {
        let mut items = vec![syn::parse_quote! {
            fn runtime_envs() -> crate::builtin::GorsSliceStorage<String> {
                stale_bodyless_fallback_marker();
                crate::builtin::GorsSliceStorage::default()
            }
        }];
        supplement_items("syscall", None, &mut items);
        let tokens = quote! { #(#items)* }.to_string();

        assert!(tokens.contains("std :: env :: vars_os"), "{tokens}");
        assert!(
            !tokens.contains("stale_bodyless_fallback_marker"),
            "{tokens}"
        );
    }

    #[test]
    fn syscall_runtime_envs_host_abi_is_exactly_root_activated() {
        let tokens = supplemented_tokens_for("syscall", &["Getuid"], Vec::new());

        assert!(!tokens.contains("runtime_envs"), "{tokens}");
        assert!(!tokens.contains("vars_os"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_lstat_from_generated_stat_shape() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Lstat"],
            vec![syn::parse_quote! {
                pub struct Stat_t {
                    pub Mode: u16,
                    pub Size: i64,
                    pub Mtimespec: Timespec,
                }
            }],
        );

        assert!(tokens.contains("pub fn Lstat"), "{tokens}");
        assert!(tokens.contains("symlink_metadata"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Mode"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Size"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Mtimespec . Sec"), "{tokens}");
        assert!(tokens.contains("raw_os_error"), "{tokens}");
        assert!(tokens.contains("Box :: new (Errno"), "{tokens}");
        assert!(tokens.contains("usize :: MAX"), "{tokens}");
        assert!(!tokens.contains("__GorsStringError"), "{tokens}");
        assert!(!tokens.contains("__gors_stat . Atimespec"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_fstat_from_borrowed_fd_and_generated_stat_shape() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Fstat"],
            vec![syn::parse_quote! {
                pub struct Stat_t {
                    pub Mode: u16,
                    pub Size: i64,
                    pub Mtimespec: Timespec,
                }
            }],
        );

        assert!(tokens.contains("pub fn Fstat"), "{tokens}");
        assert!(tokens.contains("BorrowedFd :: borrow_raw"), "{tokens}");
        assert!(tokens.contains("try_clone_to_owned"), "{tokens}");
        assert!(tokens.contains("File :: from"), "{tokens}");
        assert!(tokens.contains("File :: metadata"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Mode"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Size"), "{tokens}");
        assert!(tokens.contains("__gors_stat . Mtimespec . Sec"), "{tokens}");
        assert!(tokens.contains("raw_os_error"), "{tokens}");
        assert!(tokens.contains("Box :: new (Errno"), "{tokens}");
        assert!(!tokens.contains("symlink_metadata"), "{tokens}");
        assert!(!tokens.contains("__GorsStringError"), "{tokens}");
    }

    #[test]
    fn syscall_host_filesystem_roots_replace_only_the_requested_raw_boundary() {
        let unlink = supplemented_tokens_for(
            "syscall",
            &["Unlink"],
            vec![
                syn::parse_quote! {
                    pub fn Unlink(path: String) -> Box<dyn crate::builtin::error> {
                        stale_unlink_marker(path)
                    }
                },
                syn::parse_quote! {
                    pub fn Rmdir(path: String) -> Box<dyn crate::builtin::error> {
                        stale_rmdir_marker(path)
                    }
                },
                syn::parse_quote! {
                    pub fn Fstat(
                        fd: isize,
                        stat: crate::builtin::GorsPtr<Stat_t>,
                    ) -> Box<dyn crate::builtin::error> {
                        stale_fstat_marker(fd, stat)
                    }
                },
            ],
        );
        assert!(unlink.contains("std :: fs :: remove_file"), "{unlink}");
        assert!(!unlink.contains("stale_unlink_marker"), "{unlink}");
        assert!(unlink.contains("stale_rmdir_marker"), "{unlink}");
        assert!(unlink.contains("stale_fstat_marker"), "{unlink}");
        assert!(!unlink.contains("std :: fs :: remove_dir"), "{unlink}");

        let rmdir = supplemented_tokens_for(
            "syscall",
            &["Rmdir"],
            vec![syn::parse_quote! {
                pub fn Rmdir(path: String) -> Box<dyn crate::builtin::error> {
                    stale_rmdir_marker(path)
                }
            }],
        );
        assert!(rmdir.contains("std :: fs :: remove_dir"), "{rmdir}");
        assert!(!rmdir.contains("stale_rmdir_marker"), "{rmdir}");
        assert!(!rmdir.contains("std :: fs :: remove_file"), "{rmdir}");

        let fstat = supplemented_tokens_for(
            "syscall",
            &["Fstat"],
            vec![
                syn::parse_quote! {
                    #[derive(Default)]
                    pub struct Stat_t {
                        pub Size: i64,
                    }
                },
                syn::parse_quote! {
                    pub fn Fstat(
                        fd: isize,
                        stat: crate::builtin::GorsPtr<Stat_t>,
                    ) -> Box<dyn crate::builtin::error> {
                        stale_fstat_marker(fd, stat)
                    }
                },
            ],
        );
        assert!(fstat.contains("BorrowedFd :: borrow_raw"), "{fstat}");
        assert!(!fstat.contains("stale_fstat_marker"), "{fstat}");
        assert!(!fstat.contains("std :: fs :: remove_file"), "{fstat}");
        assert!(!fstat.contains("std :: fs :: remove_dir"), "{fstat}");
    }

    #[test]
    fn syscall_host_filesystem_errno_contract_compiles_and_runs()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["Unlink".to_string(), "Rmdir".to_string()]);
        let mut items = Vec::new();
        supplement_items("syscall", Some(&roots), &mut items);
        let builtin = syscall_host_test_builtin();
        let file: syn::File = syn::parse_quote! {
            #![allow(dead_code, non_camel_case_types, non_snake_case)]

            #builtin

            #(#items)*

            fn main() {
                let root = std::env::temp_dir().join(format!(
                    "gors-syscall-host-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                ));
                std::fs::create_dir(&root).unwrap();
                let file = root.join("file");
                std::fs::write(&file, b"gors").unwrap();

                let removed = Unlink(file.to_string_lossy().into_owned());
                assert!(removed.__gors_as_any().is_none());
                let removed = Rmdir(root.to_string_lossy().into_owned());
                assert!(removed.__gors_as_any().is_none());

                let missing = root.join("missing");
                let error = Unlink(missing.to_string_lossy().into_owned());
                let errno = error
                    .__gors_as_any()
                    .and_then(|value| value.downcast_ref::<Errno>())
                    .expect("host filesystem errors must retain Errno identity");
                assert_ne!(errno.0, 0);
                let errno_value = errno.0;
                let cloned = error.__gors_clone_box();
                let cloned_errno = cloned
                    .__gors_as_any()
                    .and_then(|value| value.downcast_ref::<Errno>())
                    .expect("cloned host errors must retain Errno identity");
                assert_eq!(cloned_errno.0, errno_value);
                let _ = error.__gors_interface_key();
            }
        };
        compile_and_run_syscall_host_test(&file)
    }

    #[test]
    fn syscall_fstat_borrows_descriptor_and_populates_generated_stat_at_runtime()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["Fstat".to_string()]);
        let mut items = vec![syn::parse_quote! {
            #[derive(Default)]
            pub struct Stat_t {
                pub Size: i64,
            }
        }];
        supplement_items("syscall", Some(&roots), &mut items);
        let builtin = syscall_host_test_builtin();
        let file: syn::File = syn::parse_quote! {
            #![allow(dead_code, non_camel_case_types, non_snake_case)]

            #builtin
            #(#items)*

            fn main() {
                let path = std::env::temp_dir().join(format!(
                    "gors-syscall-fstat-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                ));
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                std::io::Write::write_all(&mut file, b"gors").unwrap();
                let fd = std::os::fd::AsRawFd::as_raw_fd(&file) as isize;
                let stat = builtin::GorsPtr::new(Stat_t::default());

                let error = Fstat(fd, stat.clone());
                assert!(error.__gors_as_any().is_none());
                assert_eq!(stat.lock().unwrap().Size, 4);

                // The host boundary may duplicate the descriptor for metadata,
                // but must never consume the caller's descriptor ownership.
                std::io::Write::write_all(&mut file, b"!").unwrap();
                assert_eq!(std::fs::File::metadata(&file).unwrap().len(), 5);

                let invalid = Fstat(-1, stat);
                let errno = invalid
                    .__gors_as_any()
                    .and_then(|value| value.downcast_ref::<Errno>())
                    .expect("invalid descriptors must return a typed Errno");
                assert_ne!(errno.0, 0);

                drop(file);
                std::fs::remove_file(path).unwrap();
            }
        };
        compile_and_run_syscall_host_test(&file)
    }

    #[test]
    fn syscall_supplements_write_and_socklen_type_facts() {
        let mut env = TypeEnv::new();
        supplement_type_env("syscall", &mut env);

        assert_eq!(
            env.get_type_kind("_Socklen").cloned(),
            Some(TypeKind::Alias(GoType::Uint32))
        );
        assert_eq!(
            env.get_func_returns("Write"),
            vec![GoType::Int, GoType::Error]
        );
        assert_eq!(
            env.get_func_params("Write"),
            vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))]
        );
        assert_eq!(
            env.get_func_returns("write"),
            vec![GoType::Int, GoType::Error]
        );
        assert_eq!(
            env.get_func_params("write"),
            vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))]
        );
        assert_eq!(env.get_func_returns("Getuid"), vec![GoType::Int]);
        assert_eq!(env.get_func_params("Getuid"), Vec::<GoType>::new());
        assert_eq!(
            env.get_func_returns("Getenv"),
            vec![GoType::String, GoType::Bool]
        );
        assert_eq!(env.get_func_params("Getenv"), vec![GoType::String]);
        assert_eq!(
            env.get_func_returns("runtime_envs"),
            vec![GoType::Slice(Box::new(GoType::String))]
        );
        assert_eq!(env.get_func_params("runtime_envs"), Vec::<GoType>::new());
        assert_eq!(env.get_func_returns("Lstat"), vec![GoType::Error]);
        assert_eq!(
            env.get_func_params("Lstat"),
            vec![
                GoType::String,
                GoType::Pointer(Box::new(GoType::Named("Stat_t".to_string()))),
            ]
        );
        assert_eq!(env.get_func_returns("Fstat"), vec![GoType::Error]);
        assert_eq!(
            env.get_func_params("Fstat"),
            vec![
                GoType::Int,
                GoType::Pointer(Box::new(GoType::Named("Stat_t".to_string()))),
            ]
        );
        assert_eq!(env.get_func_returns("Unlink"), vec![GoType::Error]);
        assert_eq!(env.get_func_params("Unlink"), vec![GoType::String]);
        assert_eq!(env.get_func_returns("Rmdir"), vec![GoType::Error]);
        assert_eq!(env.get_func_params("Rmdir"), vec![GoType::String]);
    }

    #[test]
    fn unknown_or_unrooted_runtime_primitives_do_not_emit_modules() {
        let empty_roots = HashSet::new();

        assert!(module("runtime", None).is_none());
        assert!(module("runtime", Some(&empty_roots)).is_none());
        assert!(module("fmt", Some(&HashSet::from(["Println".to_string()]))).is_none());
    }
}
