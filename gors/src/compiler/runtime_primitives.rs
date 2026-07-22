use super::CompiledModule;
use crate::compiler::syn_inspect::{item_name, type_mentions_name};
use std::collections::{BTreeMap, HashSet};

mod os;
mod reflect;
mod sync;
mod sync_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimitiveCategory {
    CompilerIntrinsic,
    GoRuntimeAbi,
    HostResource,
    LanguagePrimitive,
}

const PRIMITIVE_CATEGORIES: &[PrimitiveCategory] = &[
    PrimitiveCategory::CompilerIntrinsic,
    PrimitiveCategory::GoRuntimeAbi,
    PrimitiveCategory::HostResource,
    PrimitiveCategory::LanguagePrimitive,
];

struct PostPrunePrimitive {
    module: &'static str,
    category: PrimitiveCategory,
    owned_symbols: &'static [&'static str],
    inject: fn(&mut CompiledModule) -> bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct PostPrunePrimitiveFacts {
    os: os::HostFileFacts,
}

impl PostPrunePrimitiveFacts {
    pub(super) fn from_os_type_env(env: Option<&crate::compiler::typeinfer::TypeEnv>) -> Self {
        Self {
            os: os::HostFileFacts::from_type_env(env),
        }
    }
}

impl PostPrunePrimitive {
    fn inject(&self, module: &mut CompiledModule) -> bool {
        debug_assert_eq!(module.mod_name, self.module);
        debug_assert!(!self.owned_symbols.is_empty());
        debug_assert!(PRIMITIVE_CATEGORIES.contains(&self.category));
        (self.inject)(module)
    }
}

const POST_PRUNE_PRIMITIVES: &[PostPrunePrimitive] = &[
    PostPrunePrimitive {
        module: reflect::MODULE,
        category: PrimitiveCategory::CompilerIntrinsic,
        owned_symbols: &["Value", "MapIter", "ValueOf", "DeepEqual"],
        inject: reflect::replace_value_module,
    },
    PostPrunePrimitive {
        module: os::MODULE,
        category: PrimitiveCategory::HostResource,
        owned_symbols: &[
            "Stdout",
            "file::__gors_resource",
            "openFileNolog",
            "runtime_rand",
            "File::{read,pread,write,pwrite,seek,readdir,readFrom,writeTo}",
            "file::close",
        ],
        inject: os::replace_host_files,
    },
    PostPrunePrimitive {
        module: sync::MODULE,
        category: PrimitiveCategory::GoRuntimeAbi,
        owned_symbols: &["Map", "Pool", "Map::*", "Pool::*"],
        inject: sync::replace_module,
    },
    PostPrunePrimitive {
        module: sync_atomic::MODULE,
        category: PrimitiveCategory::GoRuntimeAbi,
        owned_symbols: sync_atomic::OWNED_SYMBOLS,
        inject: sync_atomic::replace_module,
    },
];

pub(super) fn inject_post_prune_helpers(
    modules: &mut BTreeMap<String, CompiledModule>,
    facts: PostPrunePrimitiveFacts,
) {
    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        let changed = POST_PRUNE_PRIMITIVES
            .iter()
            .find(|primitive| primitive.module == module.mod_name)
            .is_some_and(|primitive| {
                if primitive.module == os::MODULE {
                    os::replace_host_files_with_facts(module, facts.os)
                } else {
                    primitive.inject(module)
                }
            });
        if changed {
            module.content_hash.clear();
        }
    }
}

/// Re-evaluate only host-resource ownership after the DCE pass that follows
/// initial helper injection. Temporary interface/assertion references can keep
/// an obsolete private File implementation closure alive through the first
/// replacement and disappear in that DCE pass. Replacing the exact host
/// surface once more removes that now-orphaned closure without re-running
/// unrelated reflect or synchronization primitives.
pub(super) fn stabilize_post_prune_host_helpers(
    modules: &mut BTreeMap<String, CompiledModule>,
    facts: PostPrunePrimitiveFacts,
) {
    let Some(module) = modules
        .values_mut()
        .find(|module| module.is_stdlib && module.mod_name == os::MODULE)
    else {
        return;
    };
    if os::replace_host_files_with_facts(module, facts.os) {
        module.content_hash.clear();
    }
}

pub(super) fn inject_missing_preserved_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved: &HashSet<String>,
) {
    reflect::inject_missing_value_module(modules, preserved);
}

fn module_has_struct(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == name))
}

fn module_has_static(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Static(item_static) if item_static.ident == name))
}

fn module_has_item(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| item_name(item).as_deref() == Some(name))
}

fn prune_replaced_items(
    module: &mut CompiledModule,
    item_names: &HashSet<String>,
    impl_self_type_names: &HashSet<String>,
) {
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !type_mentions_name(&item_impl.self_ty, impl_self_type_names),
        _ => item_name(item)
            .as_deref()
            .is_none_or(|name| !item_names.contains(name)),
    });
    for item in &mut module.file.items {
        crate::compiler::generated_attrs::allow_dead_code_on_item(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    fn stdlib_module(mod_name: &str, file: syn::File) -> CompiledModule {
        CompiledModule {
            mod_name: mod_name.to_string(),
            import_path: mod_name.to_string(),
            file,
            filename: format!("{mod_name}.rs"),
            content_hash: "original".to_string(),
            is_main: false,
            is_stdlib: true,
        }
    }

    fn function_body(module: &CompiledModule, name: &str) -> String {
        module
            .file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Fn(item_fn) if item_fn.sig.ident == name => {
                    Some(item_fn.block.to_token_stream().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing function {name}"))
    }

    fn inherent_method_body(module: &CompiledModule, self_name: &str, name: &str) -> String {
        module
            .file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl)
                    if item_impl.trait_.is_none()
                        && crate::compiler::syn_inspect::named_self_type(&item_impl.self_ty)
                            .as_deref()
                            == Some(self_name) =>
                {
                    Some(item_impl)
                }
                _ => None,
            })
            .flat_map(|item_impl| &item_impl.items)
            .find_map(|item| match item {
                syn::ImplItem::Fn(method) if method.sig.ident == name => {
                    Some(method.block.to_token_stream().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing method {self_name}::{name}"))
    }

    fn function_param_type(module: &CompiledModule, name: &str, index: usize) -> String {
        module
            .file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Fn(item_fn) if item_fn.sig.ident == name => item_fn
                    .sig
                    .inputs
                    .iter()
                    .nth(index)
                    .and_then(|input| match input {
                        syn::FnArg::Typed(input) => Some(input.ty.to_token_stream().to_string()),
                        syn::FnArg::Receiver(_) => None,
                    }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing function parameter {name}[{index}]"))
    }

    fn inherent_method_param_type(
        module: &CompiledModule,
        self_name: &str,
        name: &str,
        index: usize,
    ) -> String {
        module
            .file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl)
                    if item_impl.trait_.is_none()
                        && crate::compiler::syn_inspect::named_self_type(&item_impl.self_ty)
                            .as_deref()
                            == Some(self_name) =>
                {
                    Some(item_impl)
                }
                _ => None,
            })
            .flat_map(|item_impl| &item_impl.items)
            .find_map(|item| match item {
                syn::ImplItem::Fn(method) if method.sig.ident == name => method
                    .sig
                    .inputs
                    .iter()
                    .nth(index)
                    .and_then(|input| match input {
                        syn::FnArg::Typed(input) => Some(input.ty.to_token_stream().to_string()),
                        syn::FnArg::Receiver(_) => None,
                    }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing method parameter {self_name}::{name}[{index}]"))
    }

    #[test]
    fn post_prune_primitives_declare_boundaries() {
        let modules = POST_PRUNE_PRIMITIVES
            .iter()
            .map(|primitive| primitive.module)
            .collect::<HashSet<_>>();

        assert_eq!(
            modules,
            HashSet::from([
                reflect::MODULE,
                os::MODULE,
                sync::MODULE,
                sync_atomic::MODULE
            ])
        );
        for primitive in POST_PRUNE_PRIMITIVES {
            assert!(PRIMITIVE_CATEGORIES.contains(&primitive.category));
            assert!(
                !primitive.owned_symbols.is_empty(),
                "{} helper must declare owned symbols",
                primitive.module
            );
        }
    }

    fn trait_method_body(module: &CompiledModule, trait_name: &str, method_name: &str) -> String {
        module
            .file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl)
                    if item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == trait_name)
                    }) =>
                {
                    Some(item_impl)
                }
                _ => None,
            })
            .flat_map(|item_impl| &item_impl.items)
            .find_map(|item| match item {
                syn::ImplItem::Fn(method) if method.sig.ident == method_name => {
                    Some(method.block.to_token_stream().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing trait method {trait_name}::{method_name}"))
    }

    fn struct_field_names(module: &CompiledModule, name: &str) -> Vec<String> {
        module
            .file
            .items
            .iter()
            .find_map(|item| {
                let syn::Item::Struct(item_struct) = item else {
                    return None;
                };
                (item_struct.ident == name).then(|| {
                    item_struct
                        .fields
                        .iter()
                        .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                        .collect()
                })
            })
            .unwrap_or_else(|| panic!("missing struct {name}"))
    }

    fn os_host_file_fixture() -> CompiledModule {
        stdlib_module(
            "os",
            syn::parse_quote! {
                pub const PathSeparator: i32 = 47;
                pub const O_APPEND: isize = 8;
                pub const O_CREATE: isize = 16;
                pub const O_EXCL: isize = 32;
                pub const O_TRUNC: isize = 64;

                #[derive(Clone, Default)]
                #[repr(C)]
                pub struct file {
                    pfd: crate::internal__poll::FD,
                    name: String,
                    dirinfo: crate::sync__atomic::Pointer<dirInfo>,
                    nonblock: bool,
                    stdoutOrErr: bool,
                    appendMode: bool,
                    inRoot: bool,
                }

                #[derive(Clone, Default, PartialEq)]
                #[repr(C)]
                pub struct File {
                    file: crate::builtin::GorsPtr<file>,
                }

                impl std::ops::Deref for File {
                    type Target = crate::builtin::GorsPtr<file>;

                    fn deref(&self) -> &Self::Target {
                        &self.file
                    }
                }

                pub struct FileMode(pub u32);
                pub struct readdirMode(pub isize);
                pub type DirEntry = Box<dyn crate::io__fs::DirEntry>;
                pub type FileInfo = Box<dyn crate::io__fs::FileInfo>;

                pub static Stdout: crate::builtin::GorsPtr<File> = todo!();

                pub fn CreateTemp(dir: String, pattern: String) -> u64 {
                    let compiler_owned_create_temp = (dir, pattern);
                    let _ = compiler_owned_create_temp;
                    runtime_rand()
                }

                pub fn OpenFile(
                    name: String,
                    flag: isize,
                    perm: FileMode,
                ) -> (crate::builtin::GorsPtr<File>, Box<dyn crate::builtin::error>) {
                    let compiler_owned_open = (name, flag, perm);
                    let _ = compiler_owned_open;
                    todo!()
                }

                pub fn Remove(name: String) -> Box<dyn crate::builtin::error> {
                    let compiler_owned_remove = name;
                    let _ = compiler_owned_remove;
                    todo!()
                }

                fn genericNewFile(
                    name: String,
                ) -> crate::builtin::GorsPtr<File> {
                    let compiler_owned_constructor = name.clone();
                    let _ = compiler_owned_constructor;
                    crate::builtin::GorsPtr::new(File {
                        file: crate::builtin::GorsPtr::new(file {
                            pfd: Default::default(),
                            name,
                            dirinfo: Default::default(),
                            nonblock: false,
                            stdoutOrErr: false,
                            appendMode: false,
                            inRoot: false,
                        }),
                    })
                }

                fn compilerReadCaller(
                    file: crate::builtin::GorsPtr<File>,
                    mut b: Vec<u8>,
                ) -> (isize, Box<dyn crate::builtin::error>) {
                    File::Read(file, b.to_vec())
                }

                fn openFileNolog(
                    name: String,
                    flag: isize,
                    perm: FileMode,
                ) -> (crate::builtin::GorsPtr<File>, Box<dyn crate::builtin::error>) {
                    let original_raw_open = (name, flag, perm);
                    let _ = original_raw_open;
                    todo!()
                }

                fn runtime_rand() -> u64 {
                    7
                }

                impl File {
                    pub fn Name(file: crate::builtin::GorsPtr<Self>) -> String {
                        let compiler_owned_name = file;
                        let _ = compiler_owned_name;
                        String::new()
                    }

                    pub fn Read(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_read = b.len();
                        let _ = compiler_owned_read;
                        File::read(file, b.to_vec())
                    }

                    pub fn ReadAt(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                        off: i64,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_read_at = b.len();
                        let _ = compiler_owned_read_at;
                        File::pread(file, b.to_vec(), off)
                    }

                    pub fn ReadDir(
                        file: crate::builtin::GorsPtr<Self>,
                        n: isize,
                    ) -> (
                        Vec<DirEntry>,
                        Box<dyn crate::builtin::error>,
                    ) {
                        let compiler_owned_read_dir = (file, n);
                        let _ = compiler_owned_read_dir;
                        todo!()
                    }

                    pub fn ReadFrom(
                        file: crate::builtin::GorsPtr<Self>,
                        reader: &mut dyn crate::io::Reader,
                    ) -> (i64, Box<dyn crate::builtin::error>) {
                        let compiler_owned_read_from = (file, reader);
                        let _ = compiler_owned_read_from;
                        todo!()
                    }

                    pub fn Seek(
                        file: crate::builtin::GorsPtr<Self>,
                        offset: i64,
                        whence: isize,
                    ) -> (i64, Box<dyn crate::builtin::error>) {
                        let compiler_owned_seek = (file, offset, whence);
                        let _ = compiler_owned_seek;
                        todo!()
                    }

                    pub fn Stat(
                        file: crate::builtin::GorsPtr<Self>,
                    ) -> (FileInfo, Box<dyn crate::builtin::error>) {
                        let compiler_owned_stat = file;
                        let _ = compiler_owned_stat;
                        todo!()
                    }

                    pub fn Write(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_write = (file, b);
                        let _ = compiler_owned_write;
                        todo!()
                    }

                    pub fn WriteAt(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                        off: i64,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_write_at = (file, b, off);
                        let _ = compiler_owned_write_at;
                        todo!()
                    }

                    pub fn WriteString(
                        file: crate::builtin::GorsPtr<Self>,
                        value: String,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_write_string = (file, value);
                        let _ = compiler_owned_write_string;
                        todo!()
                    }

                    pub fn WriteTo(
                        file: crate::builtin::GorsPtr<Self>,
                        writer: &mut dyn crate::io::Writer,
                    ) -> (i64, Box<dyn crate::builtin::error>) {
                        let compiler_owned_write_to = (file, writer);
                        let _ = compiler_owned_write_to;
                        todo!()
                    }

                    pub fn Close(
                        file: crate::builtin::GorsPtr<Self>,
                    ) -> Box<dyn crate::builtin::error> {
                        let compiler_owned_close = file;
                        let _ = compiler_owned_close;
                        todo!()
                    }

                    fn wrapErr(
                        file: crate::builtin::GorsPtr<Self>,
                        op: String,
                        error: Box<dyn crate::builtin::error>,
                    ) -> Box<dyn crate::builtin::error> {
                        let compiler_owned_wrap = (file, op, error);
                        let _ = compiler_owned_wrap;
                        todo!()
                    }

                    fn checkValid(
                        file: crate::builtin::GorsPtr<Self>,
                        op: String,
                    ) -> Box<dyn crate::builtin::error> {
                        let compiler_owned_check = (file, op);
                        let _ = compiler_owned_check;
                        todo!()
                    }

                    fn read(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let original_raw_read = (file, b);
                        let _ = original_raw_read;
                        todo!()
                    }

                    fn pread(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                        off: i64,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let original_raw_pread = (file, b, off);
                        let _ = original_raw_pread;
                        todo!()
                    }

                    fn write(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let original_raw_write = (file, b);
                        let _ = original_raw_write;
                        todo!()
                    }

                    fn pwrite(
                        file: crate::builtin::GorsPtr<Self>,
                        b: Vec<u8>,
                        off: i64,
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let original_raw_pwrite = (file, b, off);
                        let _ = original_raw_pwrite;
                        todo!()
                    }

                    fn seek(
                        file: crate::builtin::GorsPtr<Self>,
                        offset: i64,
                        whence: isize,
                    ) -> (i64, Box<dyn crate::builtin::error>) {
                        let original_raw_seek = (file, offset, whence);
                        let _ = original_raw_seek;
                        todo!()
                    }

                    fn readFrom(
                        file: crate::builtin::GorsPtr<Self>,
                        reader: &mut dyn crate::io::Reader,
                    ) -> (i64, bool, Box<dyn crate::builtin::error>) {
                        let original_raw_read_from = (file, reader);
                        let _ = original_raw_read_from;
                        todo!()
                    }

                    fn writeTo(
                        file: crate::builtin::GorsPtr<Self>,
                        writer: &mut dyn crate::io::Writer,
                    ) -> (i64, bool, Box<dyn crate::builtin::error>) {
                        let original_raw_write_to = (file, writer);
                        let _ = original_raw_write_to;
                        todo!()
                    }

                    fn readdir(
                        file: crate::builtin::GorsPtr<Self>,
                        n: isize,
                        mode: readdirMode,
                    ) -> (
                        Vec<String>,
                        Vec<DirEntry>,
                        Vec<FileInfo>,
                        Box<dyn crate::builtin::error>,
                    ) {
                        let original_raw_readdir = (file, n, mode);
                        let _ = original_raw_readdir;
                        todo!()
                    }
                }

                impl file {
                    fn close(
                        file: crate::builtin::GorsPtr<Self>,
                    ) -> Box<dyn crate::builtin::error> {
                        let original_private_close = file;
                        let _ = original_private_close;
                        todo!()
                    }
                }

                impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                    fn Read(
                        &mut self,
                        b: &mut [u8],
                    ) -> (isize, Box<dyn crate::builtin::error>) {
                        let compiler_owned_adapter = b.len();
                        let _ = compiler_owned_adapter;
                        File::Read(self.clone(), b.to_vec())
                    }
                }
            },
        )
    }

    #[test]
    fn os_host_file_preserves_public_algorithms_and_generated_layout() {
        let mut module = os_host_file_fixture();
        let public_functions = ["CreateTemp", "OpenFile", "Remove"];
        let public_methods = [
            "Name",
            "Read",
            "ReadAt",
            "ReadDir",
            "ReadFrom",
            "Seek",
            "Stat",
            "Write",
            "WriteAt",
            "WriteString",
            "WriteTo",
            "Close",
            "wrapErr",
            "checkValid",
        ];
        let raw_methods = [
            "read", "pread", "write", "pwrite", "seek", "readFrom", "writeTo", "readdir",
        ];
        let expected_functions = public_functions
            .into_iter()
            .map(|name| (name, function_body(&module, name)))
            .collect::<BTreeMap<_, _>>();
        let expected_methods = public_methods
            .into_iter()
            .map(|name| (name, inherent_method_body(&module, "File", name)))
            .collect::<BTreeMap<_, _>>();
        let expected_raw = raw_methods
            .into_iter()
            .map(|name| (name, inherent_method_body(&module, "File", name)))
            .collect::<BTreeMap<_, _>>();
        let expected_close = inherent_method_body(&module, "file", "close");
        let expected_adapter = trait_method_body(&module, "Reader", "Read");
        let expected_deref = trait_method_body(&module, "Deref", "deref");
        let expected_file_fields = struct_field_names(&module, "File");
        let original_open = function_body(&module, "openFileNolog");
        let original_random = function_body(&module, "runtime_rand");

        assert!(os::replace_host_files(&mut module));

        for (name, body) in expected_functions {
            assert_eq!(function_body(&module, name), body, "{name}");
        }
        for (name, body) in expected_methods {
            assert_eq!(inherent_method_body(&module, "File", name), body, "{name}");
        }
        for (name, body) in expected_raw {
            assert_ne!(inherent_method_body(&module, "File", name), body, "{name}");
        }
        assert_ne!(
            inherent_method_body(&module, "file", "close"),
            expected_close
        );
        assert_ne!(function_body(&module, "openFileNolog"), original_open);
        assert_ne!(function_body(&module, "runtime_rand"), original_random);
        assert_eq!(
            trait_method_body(&module, "Reader", "Read"),
            expected_adapter
        );
        assert_eq!(trait_method_body(&module, "Deref", "deref"), expected_deref);
        assert_eq!(struct_field_names(&module, "File"), expected_file_fields);

        let private_fields = struct_field_names(&module, "file");
        for name in [
            "pfd",
            "name",
            "dirinfo",
            "nonblock",
            "stdoutOrErr",
            "appendMode",
            "inRoot",
            "__gors_resource",
        ] {
            assert!(private_fields.iter().any(|field| field == name), "{name}");
        }
        assert_eq!(
            private_fields
                .iter()
                .filter(|field| field.as_str() == "__gors_resource")
                .count(),
            1
        );
    }

    #[test]
    fn os_host_file_raw_byte_bodies_accept_owned_or_borrowed_slice_abis() {
        let mut module = os_host_file_fixture();
        for item in &mut module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            if item_impl.trait_.is_some()
                || crate::compiler::syn_inspect::named_self_type(&item_impl.self_ty).as_deref()
                    != Some("File")
            {
                continue;
            }
            for item in &mut item_impl.items {
                let syn::ImplItem::Fn(method) = item else {
                    continue;
                };
                if matches!(method.sig.ident.to_string().as_str(), "read" | "pread") {
                    let input = method.sig.inputs.iter_mut().nth(1).unwrap();
                    let syn::FnArg::Typed(input) = input else {
                        unreachable!();
                    };
                    input.ty = Box::new(syn::parse_quote! { &mut [u8] });
                }
                if matches!(method.sig.ident.to_string().as_str(), "write" | "pwrite") {
                    let input = method.sig.inputs.iter_mut().nth(1).unwrap();
                    let syn::FnArg::Typed(input) = input else {
                        unreachable!();
                    };
                    input.ty = Box::new(syn::parse_quote! { &[u8] });
                }
            }
        }

        assert!(os::replace_host_files(&mut module));
        let source = prettyplease::unparse(&module.file);
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains("b:&mut[u8]"), "{source}");
        assert!(compact.contains("letbytes:&mut[u8]=&mut*b;"), "{source}");
        assert!(compact.contains("b:&[u8]"), "{source}");
        assert!(compact.contains("letbytes:&[u8]=b.as_ref();"), "{source}");
        assert!(!source.contains("as_mut_slice"), "{source}");
        assert!(!source.contains("as_slice"), "{source}");
    }

    #[test]
    fn os_host_file_raw_read_mutation_propagates_to_callers() {
        let mut module = os_host_file_fixture();
        assert!(os::replace_host_files(&mut module));
        let mut modules = BTreeMap::from([("os".to_string(), module)]);

        super::super::call_arg_rewrites::borrow_mutated_vec_params(&mut modules);

        let module = modules.get("os").unwrap();
        let source = prettyplease::unparse(&module.file);
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(
            inherent_method_param_type(module, "File", "read", 1),
            "& mut [u8]"
        );
        assert_eq!(
            inherent_method_param_type(module, "File", "Read", 1),
            "& mut [u8]"
        );
        assert_eq!(
            function_param_type(module, "compilerReadCaller", 1),
            "& mut [u8]"
        );
        assert!(compact.contains("File::read(file,&mut*b)"), "{source}");
        assert!(compact.contains("File::Read(file,&mut*b)"), "{source}");
    }

    #[test]
    fn os_host_file_initializes_surviving_literals_and_derives_private_shape() {
        let mut module = os_host_file_fixture();
        let original_constructor = function_body(&module, "genericNewFile");

        assert!(os::replace_host_files(&mut module));

        let constructor = function_body(&module, "genericNewFile");
        assert_ne!(constructor, original_constructor);
        assert!(constructor.contains("__gors_resource"));
        assert!(constructor.contains("Default :: default"));

        let from_host = inherent_method_body(&module, "File", "__gors_from_host");
        let stdout = inherent_method_body(&module, "File", "__gors_stdout");
        for body in [&from_host, &stdout] {
            assert!(body.contains(".. Default :: default ()"), "{body}");
            assert!(!body.contains("inRoot"), "{body}");
            assert!(!body.contains("dirinfo"), "{body}");
            assert!(!body.contains("nonblock"), "{body}");
        }
        let source = prettyplease::unparse(&module.file);
        assert!(
            source.contains("std::sync::OnceLock<std::sync::Arc<__GorsHostFileResource>>"),
            "{source}"
        );
        assert!(
            source.contains("#[allow(unsafe_code)]\n    fn __gors_resource_from_raw"),
            "{source}"
        );
        assert!(!source.contains("__GorsStringError"), "{source}");
    }

    #[test]
    fn os_host_file_stabilization_is_idempotent() {
        let mut module = os_host_file_fixture();
        let expected_open = function_body(&module, "OpenFile");
        let expected_remove = function_body(&module, "Remove");

        assert!(os::replace_host_files(&mut module));
        assert!(os::replace_host_files(&mut module));

        assert_eq!(function_body(&module, "OpenFile"), expected_open);
        assert_eq!(function_body(&module, "Remove"), expected_remove);
        let source = prettyplease::unparse(&module.file);
        for declaration in [
            "struct __GorsHostFileResource",
            "struct __GorsHostDirState",
            "enum __GorsHostFileHandle",
            "fn __gors_private_file(",
            "fn __gors_private_resource(",
            "fn __gors_resource(",
            "fn __gors_path_error(",
            "fn __gors_stdout(",
            "fn openFileNolog(",
        ] {
            assert_eq!(source.matches(declaration).count(), 1, "{declaration}");
        }
        assert_eq!(
            source
                .matches("__gors_resource: Default::default()")
                .count(),
            1,
            "{source}"
        );
    }

    #[test]
    fn os_runtime_rand_only_does_not_inject_file_surface() {
        let mut module = stdlib_module(
            "os",
            syn::parse_quote! {
                fn runtime_rand() -> u64 {
                    7
                }
            },
        );

        assert!(os::replace_host_files(&mut module));
        let source = prettyplease::unparse(&module.file);
        assert!(source.contains("std::time::SystemTime"), "{source}");
        assert!(!source.contains("impl File"), "{source}");
        assert!(!source.contains("__GorsHostFileResource"), "{source}");
        assert!(!source.contains("__gors_resource"), "{source}");
    }

    #[test]
    fn os_host_file_uses_pre_prune_type_facts_for_removed_flags() {
        let mut module = os_host_file_fixture();
        module.file.items.retain(|item| {
            !matches!(
                item,
                syn::Item::Const(item_const)
                    if matches!(
                        item_const.ident.to_string().as_str(),
                        "O_APPEND" | "O_CREATE" | "O_EXCL" | "O_TRUNC"
                    )
            )
        });
        let mut env = crate::compiler::typeinfer::TypeEnv::default();
        for (name, value) in [
            ("O_APPEND", 8),
            ("O_CREATE", 16),
            ("O_EXCL", 32),
            ("O_TRUNC", 64),
        ] {
            env.set_const_integer_value(name, value);
        }
        let facts = os::HostFileFacts::from_type_env(Some(&env));

        assert!(os::replace_host_files_with_facts(&mut module, facts));
        let source = prettyplease::unparse(&module.file);
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        for expression in [
            "flag&8isize!=0",
            "flag&16isize!=0",
            "flag&32isize!=0",
            "flag&64isize!=0",
        ] {
            assert!(compact.contains(expression), "{expression}: {source}");
        }
    }

    #[test]
    fn os_host_file_unix_open_preserves_raw_flags_and_requested_access_mode() {
        let mut module = os_host_file_fixture();

        assert!(os::replace_host_files(&mut module));
        let source = function_body(&module, "openFileNolog");
        let compact = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();

        assert!(compact.contains("i32::try_from(flag&!3)"), "{source}");
        assert!(
            compact.contains("std::fs::OpenOptions::read(&mutoptions,readable);"),
            "{source}"
        );
        assert!(
            compact.contains(
                "std::os::unix::fs::OpenOptionsExt::custom_flags(&mutoptions,custom_flags"
            ),
            "{source}"
        );
        assert!(
            compact.contains("std::os::unix::fs::OpenOptionsExt::mode(&mutoptions,perm.0"),
            "{source}"
        );
        assert!(compact.contains("#[cfg(not(unix))]"), "{source}");
    }

    #[test]
    fn sync_pool_replacement_is_scoped_to_pool_modules() {
        let mut module = stdlib_module(
            "sync",
            syn::parse_quote! {
                pub struct Pool;
                pub struct Mutex;
                impl Pool {
                    pub fn old(&self) {}
                }
                impl Mutex {
                    pub fn Lock(&self) {}
                }
            },
        );

        assert!(sync::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Pool"), "{source}");
        assert!(source.contains("pub fn Get"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Mutex"), "{source}");
        assert!(source.contains("pub fn Lock"), "{source}");
    }

    #[test]
    fn sync_map_replacement_is_scoped_to_map_modules() {
        let mut module = stdlib_module(
            "sync",
            syn::parse_quote! {
                pub struct Map;
                pub struct Mutex;
                pub struct noCopy;
                impl Map {
                    pub fn old(&self) {}
                }
                impl Mutex {
                    pub fn Lock(&self) {}
                }
            },
        );

        assert!(sync::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Map"), "{source}");
        assert!(source.contains("pub fn Load"), "{source}");
        assert!(source.contains("pub fn Store"), "{source}");
        assert!(source.contains("pub fn Range"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Mutex"), "{source}");
        assert!(source.contains("pub fn Lock"), "{source}");
        assert!(source.contains("pub struct noCopy"), "{source}");
    }

    #[test]
    fn sync_atomic_replacement_preserves_requested_runtime_contract() {
        let mut module = stdlib_module(
            "sync__atomic",
            syn::parse_quote! {
                pub fn AddInt32(addr: crate::builtin::GorsPtr<i32>, delta: i32) -> i32 { 0 }
                pub fn LoadUint32(addr: crate::builtin::GorsPtr<u32>) -> u32 { 0 }
                pub fn StoreUint32(addr: crate::builtin::GorsPtr<u32>, val: u32) {}
                pub struct Int32;
                pub struct noCopy;
                pub struct Pointer<T> {
                    _blank: noCopy,
                    v: usize,
                    _marker: std::marker::PhantomData<T>,
                }
                pub struct Value;
                impl<T> Pointer<T> {
                    pub fn old(&self) {}
                }
                impl Value {
                    pub fn old(&self) {}
                }
                pub fn Keep() -> i32 { 1 }
            },
        );

        assert!(sync_atomic::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub fn AddInt32"), "{source}");
        assert!(source.contains("pub fn LoadUint32"), "{source}");
        assert!(source.contains("pub fn StoreUint32"), "{source}");
        assert!(source.contains("pub struct Int32"), "{source}");
        assert!(source.contains("pub fn And"), "{source}");
        assert!(source.contains("pub fn Or"), "{source}");
        assert!(source.contains("wrapping_add"), "{source}");
        assert!(source.contains("pub struct Pointer"), "{source}");
        assert!(source.contains("GorsPtr<T>"), "{source}");
        assert!(source.contains("pub fn CompareAndSwap"), "{source}");
        assert!(source.contains("pub fn Swap"), "{source}");
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(source.contains("pub fn Load"), "{source}");
        assert!(source.contains("pub fn Store"), "{source}");
        assert!(source.contains("pub fn Keep"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
    }

    #[test]
    fn sync_atomic_replacement_triggers_on_function_only_roots() {
        let mut module = stdlib_module(
            "sync__atomic",
            syn::parse_quote! {
                pub fn LoadUint32(addr: crate::builtin::GorsPtr<u32>) -> u32 { 0 }
                pub fn StoreUint32(addr: crate::builtin::GorsPtr<u32>, val: u32) {}
                pub fn Keep() -> i32 { 1 }
            },
        );

        assert!(sync_atomic::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub fn LoadUint32"), "{source}");
        assert!(source.contains("*value"), "{source}");
        assert!(source.contains("pub fn StoreUint32"), "{source}");
        assert!(source.contains("*value = val"), "{source}");
        assert!(source.contains("pub fn Keep"), "{source}");
    }

    #[test]
    fn reflect_value_module_injection_is_owned_by_runtime_primitives() {
        let mut modules = BTreeMap::new();
        let preserved = HashSet::from(["reflect".to_string()]);

        inject_missing_preserved_modules(&mut modules, &preserved);

        let module = modules.get("reflect");
        assert!(module.is_some(), "expected reflect module");
        let Some(module) = module else {
            return;
        };
        assert_eq!(module.mod_name, "reflect");
        assert!(module.is_stdlib);
        let source = prettyplease::unparse(&module.file);
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(
            source.contains("#[derive(Clone, Default, PartialEq)]"),
            "{source}"
        );

        inject_missing_preserved_modules(&mut modules, &preserved);

        assert_eq!(modules.len(), 1);
    }

    #[test]
    fn reflect_value_replacement_preserves_unrelated_items() {
        let mut module = stdlib_module(
            "reflect",
            syn::parse_quote! {
                pub type Kind = isize;
                pub const Slice: Kind = 23;
                pub struct Value;
                pub trait Type {
                    fn String(&mut self) -> String;
                }
                #[derive(Default)]
                pub struct __GorsNoopType;
                impl Type for __GorsNoopType {
                    fn String(&mut self) -> String {
                        String::new()
                    }
                }
                impl Value {
                    pub fn old(&self) {}
                }
                pub struct MapIter;
                impl MapIter {
                    pub fn Key(&mut self) -> Value {
                        copyVal()
                    }
                    pub fn Next(&mut self) -> bool {
                        true
                    }
                }
                pub fn copyVal() -> Value {
                    Value
                }
                pub fn ValueOf(value: Box<dyn std::any::Any>) -> Value {
                    let _ = value;
                    Value
                }
                fn deepValueEqual(v1: Value, v2: Value) -> bool {
                    let _ = (v1, v2);
                    true
                }
                pub fn DeepEqual(x: Box<dyn std::any::Any>, y: Box<dyn std::any::Any>) -> bool {
                    deepValueEqual(ValueOf(x), ValueOf(y))
                }
                pub fn KeepKind() -> Kind {
                    Slice
                }
            },
        );

        assert!(reflect::replace_value_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub type Kind"), "{source}");
        assert!(source.contains("pub const Slice"), "{source}");
        assert!(source.contains("pub trait Type"), "{source}");
        assert!(source.contains("pub struct __GorsNoopType"), "{source}");
        assert!(source.contains("pub struct MapIter"), "{source}");
        assert!(source.contains("pub fn Next"), "{source}");
        assert!(source.contains("pub fn Key"), "{source}");
        assert!(source.contains("pub fn MapRange"), "{source}");
        assert!(source.contains("pub fn KeepKind"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(source.contains("pub fn IsValid"), "{source}");
        assert!(source.contains("pub fn Type"), "{source}");
        assert!(!source.contains("pub fn copyVal"), "{source}");
        assert!(source.contains("pub fn ValueOf"), "{source}");
        assert!(!source.contains("fn deepValueEqual"), "{source}");
        assert!(source.contains("pub fn DeepEqual"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
    }
}
