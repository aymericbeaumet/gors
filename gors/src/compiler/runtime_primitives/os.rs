use super::{CompiledModule, module_has_item, module_has_static, module_has_struct};
use crate::compiler::syn_inspect::{item_name, named_self_type};
use crate::generated_names::{
    as_any_method_ident, clone_box_method_ident, interface_key_method_ident,
};
use std::collections::{BTreeMap, HashSet};

pub(super) const MODULE: &str = "os";
const FILE_TYPE: &str = "File";
const PRIVATE_FILE_TYPE: &str = "file";
const STDOUT_STATIC: &str = "Stdout";
const OPEN_FILE_NOLOG_FUNC: &str = "openFileNolog";
const RUNTIME_RAND_FUNC: &str = "runtime_rand";
const HOST_FILE_REPRESENTATION_ITEMS: &[&str] = &[
    "__GorsHostDirEntry",
    "__GorsHostDirState",
    "__GorsHostFileHandle",
    "__GorsHostFileInfo",
    "__GorsHostFileResource",
    "__GORS_HOST_RANDOM_COUNTER",
];

/// Methods whose implementation is inseparable from the host file resource.
///
/// Keep source-level wrappers such as `ReadFrom`, `WriteTo`, and `WriteString`
/// out of this list. They are ordinary Go and must continue through the generic
/// compiler path, using the private capability methods below as their host
/// boundary.
const RAW_HOST_FILE_METHODS: &[&str] = &[
    "pread", "pwrite", "read", "readFrom", "readdir", "seek", "write", "writeTo",
];
const RAW_HOST_PRIVATE_FILE_METHODS: &[&str] = &["close"];

/// Private methods emitted by this host replacement itself.
///
/// The replacement runs once after the DCE pass following initial host
/// injection. Keep that stabilization idempotent by removing only the exact
/// helper ABI owned here before regenerating it. Ordinary private methods from
/// the compiled Go package remain untouched.
const HOST_FILE_HELPER_METHODS: &[&str] = &[
    "__gors_access_error",
    "__gors_file_info",
    "__gors_from_host",
    "__gors_go_path",
    "__gors_host_error",
    "__gors_host_path",
    "__gors_io_error",
    "__gors_nil_error",
    "__gors_path_error",
    "__gors_private_file",
    "__gors_private_resource",
    "__gors_resource",
    "__gors_resource_from_raw",
    "__gors_stdout",
];

#[derive(Default)]
struct HostFileSurface {
    file: bool,
    private_file: bool,
    stdout: bool,
    open_file_nolog: bool,
    runtime_rand: bool,
    append_flag: Option<isize>,
    create_flag: Option<isize>,
    exclusive_flag: Option<isize>,
    truncate_flag: Option<isize>,
    methods: BTreeMap<String, syn::ImplItemFn>,
    private_file_methods: BTreeMap<String, syn::ImplItemFn>,
    invalidated_private_items: HashSet<String>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct HostFileFacts {
    append_flag: Option<isize>,
    create_flag: Option<isize>,
    exclusive_flag: Option<isize>,
    truncate_flag: Option<isize>,
}

impl HostFileFacts {
    pub(super) fn from_type_env(env: Option<&crate::compiler::typeinfer::TypeEnv>) -> Self {
        let flag = |name: &str| {
            env.and_then(|env| env.get_const_integer_value(name))
                .and_then(|value| isize::try_from(value).ok())
        };
        Self {
            append_flag: flag("O_APPEND"),
            create_flag: flag("O_CREATE"),
            exclusive_flag: flag("O_EXCL"),
            truncate_flag: flag("O_TRUNC"),
        }
    }

    fn from_module(module: &CompiledModule) -> Self {
        Self {
            append_flag: module_integer_const_value(module, "O_APPEND"),
            create_flag: module_integer_const_value(module, "O_CREATE"),
            exclusive_flag: module_integer_const_value(module, "O_EXCL"),
            truncate_flag: module_integer_const_value(module, "O_TRUNC"),
        }
    }
}

impl HostFileSurface {
    fn collect(module: &CompiledModule, facts: HostFileFacts) -> Self {
        let mut methods = BTreeMap::new();
        let mut private_file_methods = BTreeMap::new();
        for item in &module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            if item_impl.trait_.is_some() {
                continue;
            }
            let self_name = named_self_type(&item_impl.self_ty);
            if !matches!(
                self_name.as_deref(),
                Some(FILE_TYPE) | Some(PRIVATE_FILE_TYPE)
            ) {
                continue;
            }
            for item in &item_impl.items {
                let syn::ImplItem::Fn(method) = item else {
                    continue;
                };
                let destination = if self_name.as_deref() == Some(FILE_TYPE) {
                    &mut methods
                } else {
                    &mut private_file_methods
                };
                destination
                    .entry(method.sig.ident.to_string())
                    .or_insert_with(|| method.clone());
            }
        }
        let raw_methods = methods
            .iter()
            .filter(|(name, _)| RAW_HOST_FILE_METHODS.contains(&name.as_str()))
            .map(|(name, method)| (name.clone(), method.clone()))
            .collect::<BTreeMap<_, _>>();
        let replaced_dependency_names = replaced_file_dependency_names(module, &raw_methods);
        let invalidated_private_items = invalidated_replaced_file_dependency_names(
            module,
            &replaced_dependency_names,
            &raw_methods,
        );
        let open_file_nolog = module_has_item(module, OPEN_FILE_NOLOG_FUNC);
        Self {
            file: module_has_struct(module, FILE_TYPE),
            private_file: module_has_struct(module, PRIVATE_FILE_TYPE),
            stdout: module_has_static(module, STDOUT_STATIC),
            open_file_nolog,
            runtime_rand: module_has_item(module, RUNTIME_RAND_FUNC),
            append_flag: facts.append_flag,
            create_flag: facts.create_flag,
            exclusive_flag: facts.exclusive_flag,
            truncate_flag: facts.truncate_flag,
            methods,
            private_file_methods,
            invalidated_private_items,
        }
    }

    fn is_empty(&self) -> bool {
        !self.file
            && !self.private_file
            && !self.stdout
            && !self.open_file_nolog
            && !self.runtime_rand
            && self.methods.is_empty()
    }

    fn has_method(&self, name: &str) -> bool {
        self.methods.contains_key(name)
    }

    fn method(&self, name: &str) -> Option<&syn::ImplItemFn> {
        self.methods.get(name)
    }

    fn private_file_method(&self, name: &str) -> Option<&syn::ImplItemFn> {
        self.private_file_methods.get(name)
    }

    fn has_host_file_boundary(&self) -> bool {
        self.file
            && self.private_file
            && (self.stdout
                || self.open_file_nolog
                || RAW_HOST_FILE_METHODS
                    .iter()
                    .any(|name| self.has_method(name))
                || RAW_HOST_PRIVATE_FILE_METHODS
                    .iter()
                    .any(|name| self.private_file_method(name).is_some()))
    }
}

fn module_integer_const_value(module: &CompiledModule, name: &str) -> Option<isize> {
    fn expr_value(expr: &syn::Expr) -> Option<i128> {
        match expr {
            syn::Expr::Lit(literal) => match &literal.lit {
                syn::Lit::Int(value) => value.base10_parse().ok(),
                _ => None,
            },
            syn::Expr::Paren(paren) => expr_value(&paren.expr),
            syn::Expr::Group(group) => expr_value(&group.expr),
            syn::Expr::Unary(unary) => match unary.op {
                syn::UnOp::Neg(_) => expr_value(&unary.expr)?.checked_neg(),
                syn::UnOp::Not(_) => Some(!expr_value(&unary.expr)?),
                _ => None,
            },
            _ => None,
        }
    }

    module.file.items.iter().find_map(|item| {
        let syn::Item::Const(item_const) = item else {
            return None;
        };
        (item_const.ident == name)
            .then(|| expr_value(&item_const.expr))
            .flatten()
            .and_then(|value| isize::try_from(value).ok())
    })
}

struct LocalNameMentionCollector<'a> {
    local_names: &'a HashSet<String>,
    mentioned: HashSet<String>,
}

impl syn::visit::Visit<'_> for LocalNameMentionCollector<'_> {
    fn visit_path(&mut self, path: &syn::Path) {
        self.mentioned.extend(
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .filter(|name| self.local_names.contains(name)),
        );
        syn::visit::visit_path(self, path);
    }
}

fn replaced_file_dependency_names(
    module: &CompiledModule,
    methods: &BTreeMap<String, syn::ImplItemFn>,
) -> HashSet<String> {
    let local_names = module
        .file
        .items
        .iter()
        .flat_map(item_owned_names)
        .collect::<HashSet<_>>();
    let mut dependencies = HashSet::new();
    for method in methods.values() {
        dependencies.extend(method_local_name_mentions(method, &local_names));
    }
    dependencies.extend(private_preserved_implementors_for_methods(module, methods));

    let mut visited = HashSet::new();
    loop {
        let mut changed = false;
        for (index, item) in module.file.items.iter().enumerate() {
            if visited.contains(&index)
                || !item_owned_names(item)
                    .iter()
                    .any(|name| dependencies.contains(name))
            {
                continue;
            }
            visited.insert(index);
            let before = dependencies.len();
            dependencies.extend(item_local_name_mentions(item, &local_names));
            changed |= dependencies.len() != before;
        }
        if !changed {
            break;
        }
    }
    dependencies
}

fn private_preserved_implementors_for_methods(
    module: &CompiledModule,
    methods: &BTreeMap<String, syn::ImplItemFn>,
) -> HashSet<String> {
    let preserved = module
        .file
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            if !crate::compiler::generated_attrs::attrs_preserve_for_dce(&item_impl.attrs) {
                return None;
            }
            let (_, trait_path, _) = item_impl.trait_.as_ref()?;
            let trait_name = trait_path.segments.last()?.ident.to_string();
            let self_name = named_self_type(&item_impl.self_ty)?;
            (!self_name.chars().next().is_some_and(char::is_uppercase))
                .then_some((trait_name, self_name))
        })
        .collect::<Vec<_>>();
    let trait_names = preserved
        .iter()
        .map(|(trait_name, _)| trait_name.clone())
        .collect::<HashSet<_>>();
    let mentioned_traits = methods
        .values()
        .flat_map(|method| method_local_name_mentions(method, &trait_names))
        .collect::<HashSet<_>>();

    preserved
        .into_iter()
        .filter_map(|(trait_name, self_name)| {
            mentioned_traits.contains(&trait_name).then_some(self_name)
        })
        .collect()
}

fn item_owned_names(item: &syn::Item) -> Vec<String> {
    let mut names = item_name(item).into_iter().collect::<Vec<_>>();
    if let syn::Item::Impl(item_impl) = item
        && let Some(name) = named_self_type(&item_impl.self_ty)
        && !names.contains(&name)
    {
        names.push(name);
    }
    names
}

fn method_local_name_mentions(
    method: &syn::ImplItemFn,
    local_names: &HashSet<String>,
) -> HashSet<String> {
    let mut collector = LocalNameMentionCollector {
        local_names,
        mentioned: HashSet::new(),
    };
    syn::visit::Visit::visit_impl_item_fn(&mut collector, method);
    collector.mentioned
}

fn item_local_name_mentions(item: &syn::Item, local_names: &HashSet<String>) -> HashSet<String> {
    let mut collector = LocalNameMentionCollector {
        local_names,
        mentioned: HashSet::new(),
    };
    syn::visit::Visit::visit_item(&mut collector, item);
    collector.mentioned
}

fn item_belongs_to_dependency_closure(
    item: &syn::Item,
    dependency_names: &HashSet<String>,
) -> bool {
    item_owned_names(item)
        .iter()
        .any(|name| dependency_names.contains(name))
}

fn invalidated_replaced_file_dependency_names(
    module: &CompiledModule,
    dependency_names: &HashSet<String>,
    replaced_methods: &BTreeMap<String, syn::ImplItemFn>,
) -> HashSet<String> {
    let mut retained = dependency_names
        .iter()
        .filter(|name| {
            name.as_str() == PRIVATE_FILE_TYPE
                || (name.as_str() != FILE_TYPE
                    && name.chars().next().is_some_and(char::is_uppercase))
        })
        .cloned()
        .collect::<HashSet<_>>();

    for item in &module.file.items {
        if let syn::Item::Impl(item_impl) = item
            && crate::compiler::generated_attrs::attrs_preserve_for_dce(&item_impl.attrs)
        {
            retained.extend(
                item_owned_names(item)
                    .into_iter()
                    .filter(|name| dependency_names.contains(name)),
            );
            retained.extend(
                item_local_name_mentions(item, dependency_names)
                    .into_iter()
                    .filter(|name| dependency_names.contains(name)),
            );
            continue;
        }
        if let syn::Item::Impl(item_impl) = item
            && item_impl.trait_.is_none()
            && named_self_type(&item_impl.self_ty).as_deref() == Some(FILE_TYPE)
        {
            for method in item_impl.items.iter().filter_map(|item| match item {
                syn::ImplItem::Fn(method)
                    if !replaced_methods.contains_key(&method.sig.ident.to_string()) =>
                {
                    Some(method)
                }
                _ => None,
            }) {
                retained.extend(
                    method_local_name_mentions(method, dependency_names)
                        .into_iter()
                        .filter(|name| dependency_names.contains(name)),
                );
            }
            continue;
        }
        if item_belongs_to_dependency_closure(item, dependency_names) {
            continue;
        }
        retained.extend(
            item_local_name_mentions(item, dependency_names)
                .into_iter()
                .filter(|name| dependency_names.contains(name)),
        );
    }

    loop {
        let mut changed = false;
        for item in &module.file.items {
            if !item_owned_names(item)
                .iter()
                .any(|name| retained.contains(name))
            {
                continue;
            }
            let before = retained.len();
            retained.extend(
                item_local_name_mentions(item, dependency_names)
                    .into_iter()
                    .filter(|name| dependency_names.contains(name)),
            );
            changed |= retained.len() != before;
        }
        if !changed {
            break;
        }
    }

    dependency_names
        .iter()
        .filter(|name| {
            !retained.contains(*name) && !name.chars().next().is_some_and(char::is_uppercase)
        })
        .cloned()
        .collect()
}

pub(super) fn replace_host_files(module: &mut CompiledModule) -> bool {
    let facts = HostFileFacts::from_module(module);
    replace_host_files_with_facts(module, facts)
}

pub(super) fn replace_host_files_with_facts(
    module: &mut CompiledModule,
    facts: HostFileFacts,
) -> bool {
    let surface = HostFileSurface::collect(module, facts);
    if surface.is_empty() {
        return false;
    }

    prune_owned_host_file_items(module, &surface);
    if surface.has_host_file_boundary() {
        inject_host_file_types(module, &surface);
        inject_host_file_methods(module, &surface);
        inject_host_private_file_methods(module, &surface);
    }
    inject_host_file_functions(module, &surface);
    true
}

fn prune_owned_host_file_items(module: &mut CompiledModule, surface: &HostFileSurface) {
    let mut owned_items = HashSet::from([
        STDOUT_STATIC.to_string(),
        OPEN_FILE_NOLOG_FUNC.to_string(),
        RUNTIME_RAND_FUNC.to_string(),
    ]);
    owned_items.extend(
        HOST_FILE_REPRESENTATION_ITEMS
            .iter()
            .map(ToString::to_string),
    );
    let mut retained = Vec::with_capacity(module.file.items.len());
    for item in std::mem::take(&mut module.file.items) {
        if item_name(&item).is_some_and(|name| surface.invalidated_private_items.contains(&name)) {
            continue;
        }
        let syn::Item::Impl(mut item_impl) = item else {
            if item_name(&item)
                .as_deref()
                .is_none_or(|name| !owned_items.contains(name))
            {
                retained.push(item);
            }
            continue;
        };
        let Some(self_name) = named_self_type(&item_impl.self_ty) else {
            retained.push(syn::Item::Impl(item_impl));
            continue;
        };
        if surface.invalidated_private_items.contains(&self_name) {
            continue;
        }
        if HOST_FILE_REPRESENTATION_ITEMS.contains(&self_name.as_str()) {
            continue;
        }
        if self_name == PRIVATE_FILE_TYPE && item_impl.trait_.is_none() {
            item_impl.items.retain(|item| {
                !matches!(
                    item,
                    syn::ImplItem::Fn(method)
                        if RAW_HOST_PRIVATE_FILE_METHODS
                            .contains(&method.sig.ident.to_string().as_str())
                )
            });
            if !item_impl.items.is_empty() {
                retained.push(syn::Item::Impl(item_impl));
            }
            continue;
        }
        if self_name != FILE_TYPE {
            retained.push(syn::Item::Impl(item_impl));
            continue;
        }
        if item_impl.trait_.is_some() {
            retained.push(syn::Item::Impl(item_impl));
            continue;
        }

        // One generated impl can contain both raw host-facing methods and
        // ordinary Go wrappers. Remove only the exact raw methods that this
        // helper owns; preserving the rest keeps their source algorithms and
        // any unrelated future File API on the generic compiler path.
        item_impl.items.retain(|item| {
            !matches!(
                item,
                syn::ImplItem::Fn(method)
                    if RAW_HOST_FILE_METHODS.contains(&method.sig.ident.to_string().as_str())
                        || HOST_FILE_HELPER_METHODS
                            .contains(&method.sig.ident.to_string().as_str())
            )
        });
        if !item_impl.items.is_empty() {
            retained.push(syn::Item::Impl(item_impl));
        }
    }
    module.file.items = retained;
    for item in &mut module.file.items {
        crate::compiler::generated_attrs::allow_dead_code_on_item(item);
    }
}

fn initialize_private_file_resource_fields(file: &mut syn::File) {
    struct Initializer;

    impl syn::visit_mut::VisitMut for Initializer {
        fn visit_expr_struct_mut(&mut self, expr: &mut syn::ExprStruct) {
            syn::visit_mut::visit_expr_struct_mut(self, expr);
            if expr
                .path
                .segments
                .last()
                .is_none_or(|segment| segment.ident != PRIVATE_FILE_TYPE)
                || expr.fields.iter().any(|field| {
                    matches!(
                        &field.member,
                        syn::Member::Named(ident) if ident == "__gors_resource"
                    )
                })
            {
                return;
            }
            expr.fields.push(syn::parse_quote! {
                __gors_resource: Default::default()
            });
        }
    }

    syn::visit_mut::VisitMut::visit_file_mut(&mut Initializer, file);
}

fn inject_host_file_types(module: &mut CompiledModule, surface: &HostFileSurface) {
    // The private Go type can be constructed by generic SDK code outside the
    // raw methods replaced below. Initialize the sidecar in every surviving
    // literal before extending the struct so no constructor is specialized or
    // invalidated by the host boundary.
    initialize_private_file_resource_fields(&mut module.file);
    for item in &mut module.file.items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != PRIVATE_FILE_TYPE {
            continue;
        }
        let syn::Fields::Named(fields) = &mut item_struct.fields else {
            continue;
        };
        if !fields.named.iter().any(|field| {
            field
                .ident
                .as_ref()
                .is_some_and(|ident| ident == "__gors_resource")
        }) {
            fields.named.push(syn::parse_quote! {
                __gors_resource: std::sync::OnceLock<std::sync::Arc<__GorsHostFileResource>>
            });
        }
    }

    module.file.items.extend([
        syn::parse_quote! {
            #[allow(dead_code)]
            struct __GorsHostFileResource {
                name: String,
                handle: std::sync::Mutex<__GorsHostFileHandle>,
                directory: std::sync::Mutex<Option<__GorsHostDirState>>,
                append_mode: bool,
                readable: bool,
                writable: bool,
            }
        },
        syn::parse_quote! {
            #[allow(dead_code)]
            struct __GorsHostDirState {
                entries: Vec<std::path::PathBuf>,
                next: usize,
            }
        },
        syn::parse_quote! {
            #[allow(dead_code)]
            enum __GorsHostFileHandle {
                Stdout,
                Host(std::fs::File),
                Closed,
            }
        },
        syn::parse_quote! {
            impl Default for __GorsHostFileResource {
                fn default() -> Self {
                    Self {
                        name: String::new(),
                        handle: std::sync::Mutex::new(__GorsHostFileHandle::Closed),
                        directory: std::sync::Mutex::new(None),
                        append_mode: false,
                        readable: false,
                        writable: false,
                    }
                }
            }
        },
    ]);

    if surface.stdout {
        module.file.items.push(syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            pub static Stdout: std::sync::LazyLock<crate::builtin::GorsPtr<File>> =
                std::sync::LazyLock::new(File::__gors_stdout);
        });
    }

    if surface.has_method("readdir") {
        let as_any = as_any_method_ident();
        let interface_key = interface_key_method_ident();
        let clone_box = clone_box_method_ident();
        module.file.items.extend([
            syn::parse_quote! {
                #[derive(Clone)]
                #[allow(dead_code)]
                struct __GorsHostFileInfo {
                    name: String,
                    size: i64,
                    mode: crate::io__fs::FileMode,
                    modified_sec: i64,
                    modified_nsec: i64,
                    is_dir: bool,
                }
            },
            syn::parse_quote! {
                impl crate::io__fs::FileInfo for __GorsHostFileInfo {
                    fn #as_any(&self) -> Option<&dyn std::any::Any> {
                        Some(self)
                    }

                    fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey {
                        crate::builtin::GorsInterfaceKey::non_comparable::<Self>()
                    }

                    fn #clone_box(&self) -> Box<dyn crate::io__fs::FileInfo> {
                        Box::new(self.clone()) as Box<dyn crate::io__fs::FileInfo>
                    }

                    fn Name(&mut self) -> String {
                        self.name.clone()
                    }

                    fn Size(&mut self) -> i64 {
                        self.size
                    }

                    fn Mode(&mut self) -> crate::io__fs::FileMode {
                        self.mode
                    }

                    fn ModTime(&mut self) -> crate::time::Time {
                        crate::time::Unix(self.modified_sec, self.modified_nsec)
                    }

                    fn IsDir(&mut self) -> bool {
                        self.is_dir
                    }

                    fn Sys(&mut self) -> Box<dyn std::any::Any> {
                        Box::new(()) as Box<dyn std::any::Any>
                    }
                }
            },
        ]);
    }

    if surface.has_method("readdir") {
        let as_any = as_any_method_ident();
        let interface_key = interface_key_method_ident();
        let clone_box = clone_box_method_ident();
        module.file.items.extend([
            syn::parse_quote! {
                #[derive(Clone)]
                #[allow(dead_code)]
                struct __GorsHostDirEntry {
                    info: __GorsHostFileInfo,
                    type_mode: crate::io__fs::FileMode,
                }
            },
            syn::parse_quote! {
                impl crate::io__fs::DirEntry for __GorsHostDirEntry {
                    fn #as_any(&self) -> Option<&dyn std::any::Any> {
                        Some(self)
                    }

                    fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey {
                        crate::builtin::GorsInterfaceKey::non_comparable::<Self>()
                    }

                    fn #clone_box(&self) -> Box<dyn crate::io__fs::DirEntry> {
                        Box::new(self.clone()) as Box<dyn crate::io__fs::DirEntry>
                    }

                    fn Name(&mut self) -> String {
                        self.info.name.clone()
                    }

                    fn IsDir(&mut self) -> bool {
                        self.info.is_dir
                    }

                    fn Type(&mut self) -> crate::io__fs::FileMode {
                        self.type_mode
                    }

                    fn Info(
                        &mut self,
                    ) -> (
                        Box<dyn crate::io__fs::FileInfo>,
                        Box<dyn crate::builtin::error>,
                    ) {
                        (
                            Box::new(self.info.clone()) as Box<dyn crate::io__fs::FileInfo>,
                            File::__gors_nil_error(),
                        )
                    }
                }
            },
        ]);
    }

    if surface.runtime_rand {
        module.file.items.push(syn::parse_quote! {
            #[allow(dead_code)]
            static __GORS_HOST_RANDOM_COUNTER: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
        });
    }
}

fn preserve_compiled_method_signature(
    original: &syn::ImplItemFn,
    replacement: syn::ImplItemFn,
) -> syn::ImplItemFn {
    if original.sig.inputs.len() != replacement.sig.inputs.len() {
        return replacement;
    }
    if original
        .sig
        .inputs
        .iter()
        .zip(&replacement.sig.inputs)
        .any(|(original, replacement)| {
            !matches!(
                (original, replacement),
                (syn::FnArg::Receiver(_), syn::FnArg::Receiver(_))
                    | (syn::FnArg::Typed(_), syn::FnArg::Typed(_))
            )
        })
    {
        return replacement;
    }

    let mut method = original.clone();
    for (original, replacement) in method.sig.inputs.iter_mut().zip(&replacement.sig.inputs) {
        if let (syn::FnArg::Typed(original), syn::FnArg::Typed(replacement)) =
            (original, replacement)
        {
            original.pat = replacement.pat.clone();
        }
    }
    method.block = replacement.block;
    method
}

fn inject_host_file_methods(module: &mut CompiledModule, surface: &HostFileSurface) {
    let mut item_impl: syn::ItemImpl = syn::parse_quote! {
        #[allow(dead_code)]
        impl File {
            fn __gors_private_file(
                file: &crate::builtin::GorsPtr<Self>,
            ) -> crate::builtin::GorsPtr<file> {
                match file.lock() {
                    Ok(file) => file.file.clone(),
                    Err(error) => crate::builtin::panic_value(error),
                }
            }

            fn __gors_resource(
                file: &crate::builtin::GorsPtr<Self>,
            ) -> std::sync::Arc<__GorsHostFileResource> {
                let private = Self::__gors_private_file(file);
                Self::__gors_private_resource(&private)
            }

            fn __gors_private_resource(
                private: &crate::builtin::GorsPtr<file>,
            ) -> std::sync::Arc<__GorsHostFileResource> {
                match private.lock() {
                    Ok(private) => private
                        .__gors_resource
                        .get_or_init(|| {
                            Self::__gors_resource_from_raw(
                                private.name.clone(),
                                private.pfd.Sysfd,
                                private.appendMode,
                            )
                        })
                        .clone(),
                    Err(error) => crate::builtin::panic_value(error),
                }
            }

            #[allow(unsafe_code)]
            fn __gors_resource_from_raw(
                name: String,
                raw_fd: isize,
                append_mode: bool,
            ) -> std::sync::Arc<__GorsHostFileResource> {
                #[cfg(unix)]
                let handle = if raw_fd >= 0 {
                    use std::os::fd::FromRawFd;
                    __GorsHostFileHandle::Host(unsafe {
                        std::fs::File::from_raw_fd(raw_fd as std::os::fd::RawFd)
                    })
                } else {
                    __GorsHostFileHandle::Closed
                };
                #[cfg(not(unix))]
                let handle = {
                    let _ = raw_fd;
                    __GorsHostFileHandle::Closed
                };
                std::sync::Arc::new(__GorsHostFileResource {
                    name,
                    handle: std::sync::Mutex::new(handle),
                    directory: std::sync::Mutex::new(None),
                    append_mode,
                    readable: true,
                    writable: true,
                })
            }

            fn __gors_nil_error() -> Box<dyn crate::builtin::error> {
                Box::new(crate::builtin::__GorsNooperror::default())
                    as Box<dyn crate::builtin::error>
            }

            fn __gors_access_error() -> Box<dyn crate::builtin::error> {
                (*ErrInvalid).clone()
            }

            fn __gors_host_error(
                error: impl std::fmt::Display,
            ) -> Box<dyn crate::builtin::error> {
                let _ = error;
                Box::new(crate::syscall::Errno(usize::MAX))
                    as Box<dyn crate::builtin::error>
            }

            fn __gors_io_error(error: &std::io::Error) -> Box<dyn crate::builtin::error> {
                let errno = error
                    .raw_os_error()
                    .map_or(usize::MAX, |errno| errno.unsigned_abs() as usize);
                Box::new(crate::syscall::Errno(errno)) as Box<dyn crate::builtin::error>
            }

            fn __gors_path_error(
                op: String,
                path: String,
                error: Box<dyn crate::builtin::error>,
            ) -> Box<dyn crate::builtin::error> {
                Box::new(crate::builtin::GorsPtr::new(crate::io__fs::PathError {
                    Op: op,
                    Path: path,
                    Err: error,
                })) as Box<dyn crate::builtin::error>
            }

            fn __gors_from_host(
                name: String,
                host: std::fs::File,
                append_mode: bool,
                readable: bool,
                writable: bool,
            ) -> Self {
                #[cfg(unix)]
                let raw_fd = {
                    use std::os::fd::AsRawFd;
                    host.as_raw_fd() as isize
                };
                #[cfg(not(unix))]
                let raw_fd = -1isize;

                let resource = std::sync::Arc::new(__GorsHostFileResource {
                    name: name.clone(),
                    handle: std::sync::Mutex::new(__GorsHostFileHandle::Host(host)),
                    directory: std::sync::Mutex::new(None),
                    append_mode,
                    readable,
                    writable,
                });
                let mut pfd = crate::internal__poll::FD::default();
                pfd.Sysfd = raw_fd;
                let resource_cell = std::sync::OnceLock::new();
                let _ = resource_cell.set(resource);
                Self {
                    file: crate::builtin::GorsPtr::new(file {
                        pfd,
                        name,
                        appendMode: append_mode,
                        __gors_resource: resource_cell,
                        ..Default::default()
                    }),
                }
            }

            fn __gors_stdout() -> crate::builtin::GorsPtr<Self> {
                let resource = std::sync::Arc::new(__GorsHostFileResource {
                    name: "/dev/stdout".to_string(),
                    handle: std::sync::Mutex::new(__GorsHostFileHandle::Stdout),
                    directory: std::sync::Mutex::new(None),
                    append_mode: false,
                    readable: false,
                    writable: true,
                });
                let mut pfd = crate::internal__poll::FD::default();
                #[cfg(unix)]
                {
                    pfd.Sysfd = 1;
                }
                let resource_cell = std::sync::OnceLock::new();
                let _ = resource_cell.set(resource);
                crate::builtin::GorsPtr::new(Self {
                    file: crate::builtin::GorsPtr::new(file {
                        pfd,
                        name: "/dev/stdout".to_string(),
                        stdoutOrErr: true,
                        __gors_resource: resource_cell,
                        ..Default::default()
                    }),
                })
            }

            fn __gors_host_path(value: &str) -> std::path::PathBuf {
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStringExt;
                    std::path::PathBuf::from(std::ffi::OsString::from_vec(
                        crate::builtin::go_string_bytes(value),
                    ))
                }
                #[cfg(not(unix))]
                {
                    std::path::PathBuf::from(value)
                }
            }

            fn __gors_go_path(value: &std::path::Path) -> String {
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    crate::builtin::go_string_from_bytes(value.as_os_str().as_bytes())
                }
                #[cfg(not(unix))]
                {
                    value.to_string_lossy().into_owned()
                }
            }
        }
    };

    if surface.has_method("readdir") {
        item_impl.items.push(syn::parse_quote! {
            fn __gors_file_info(
                name: String,
                metadata: &std::fs::Metadata,
            ) -> __GorsHostFileInfo {
                let mut mode = if metadata.is_dir() { 1_u32 << 31 } else { 0 };
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    mode |= metadata.permissions().mode() & 0o777;
                }
                let (modified_sec, modified_nsec) = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| {
                        modified
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .ok()
                    })
                    .map_or((0, 0), |duration| {
                        (duration.as_secs() as i64, i64::from(duration.subsec_nanos()))
                    });
                __GorsHostFileInfo {
                    name,
                    size: metadata.len() as i64,
                    mode: crate::io__fs::FileMode(mode),
                    modified_sec,
                    modified_nsec,
                    is_dir: metadata.is_dir(),
                }
            }
        });
    }

    for name in RAW_HOST_FILE_METHODS {
        let Some(original) = surface.method(name) else {
            continue;
        };
        let replacement: syn::ImplItemFn = match *name {
            "read" => syn::parse_quote! {
                fn read(
                    file: crate::builtin::GorsPtr<Self>,
                    mut b: crate::builtin::GorsSliceStorage<u8>,
                ) -> (isize, Box<dyn crate::builtin::error>) {
                    let resource = Self::__gors_resource(&file);
                    if !resource.readable {
                        return (0, Self::__gors_access_error());
                    }
                    let mut handle = match resource.handle.lock() {
                        Ok(handle) => handle,
                        Err(error) => return (0, Self::__gors_host_error(error)),
                    };
                    let result = match &mut *handle {
                        __GorsHostFileHandle::Host(host) => {
                            let bytes: &mut [u8] = &mut *b;
                            std::io::Read::read(host, bytes)
                        }
                        __GorsHostFileHandle::Stdout => {
                            return (0, (*ErrInvalid).clone());
                        }
                        __GorsHostFileHandle::Closed => {
                            return (0, (*ErrClosed).clone());
                        }
                    };
                    match result {
                        Ok(0) if !b.is_empty() => (0, (*crate::io::EOF).clone()),
                        Ok(read) => (read as isize, Self::__gors_nil_error()),
                        Err(error) => (0, Self::__gors_io_error(&error)),
                    }
                }
            },
            "pread" => syn::parse_quote! {
                fn pread(
                    file: crate::builtin::GorsPtr<Self>,
                    mut b: crate::builtin::GorsSliceStorage<u8>,
                    off: i64,
                ) -> (isize, Box<dyn crate::builtin::error>) {
                    if off < 0 {
                        return (0, (*ErrInvalid).clone());
                    }
                    let resource = Self::__gors_resource(&file);
                    if !resource.readable {
                        return (0, Self::__gors_access_error());
                    }
                    let handle = match resource.handle.lock() {
                        Ok(handle) => handle,
                        Err(error) => return (0, Self::__gors_host_error(error)),
                    };
                    let __GorsHostFileHandle::Host(host) = &*handle else {
                        return (0, (*ErrClosed).clone());
                    };
                    #[cfg(unix)]
                    let result = {
                        use std::os::unix::fs::FileExt;
                        let bytes: &mut [u8] = &mut *b;
                        host.read_at(bytes, off as u64)
                    };
                    #[cfg(windows)]
                    let result = {
                        use std::os::windows::fs::FileExt;
                        let bytes: &mut [u8] = &mut *b;
                        host.seek_read(bytes, off as u64)
                    };
                    #[cfg(not(any(unix, windows)))]
                    let result: std::io::Result<usize> = Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "positional reads are unsupported on this host",
                    ));
                    match result {
                        Ok(0) if !b.is_empty() => (0, (*crate::io::EOF).clone()),
                        Ok(read) => (read as isize, Self::__gors_nil_error()),
                        Err(error) => (0, Self::__gors_io_error(&error)),
                    }
                }
            },
            "write" => syn::parse_quote! {
                fn write(
                    file: crate::builtin::GorsPtr<Self>,
                    b: crate::builtin::GorsSliceStorage<u8>,
                ) -> (isize, Box<dyn crate::builtin::error>) {
                    let resource = Self::__gors_resource(&file);
                    if !resource.writable {
                        return (0, Self::__gors_access_error());
                    }
                    let mut handle = match resource.handle.lock() {
                        Ok(handle) => handle,
                        Err(error) => return (0, Self::__gors_host_error(error)),
                    };
                    let result = match &mut *handle {
                        __GorsHostFileHandle::Host(host) => {
                            let bytes: &[u8] = b.as_ref();
                            std::io::Write::write(host, bytes)
                        }
                        __GorsHostFileHandle::Stdout => {
                            let mut stdout = std::io::stdout();
                            let bytes: &[u8] = b.as_ref();
                            std::io::Write::write(&mut stdout, bytes)
                        }
                        __GorsHostFileHandle::Closed => {
                            return (0, (*ErrClosed).clone());
                        }
                    };
                    match result {
                        Ok(written) => (written as isize, Self::__gors_nil_error()),
                        Err(error) => (0, Self::__gors_io_error(&error)),
                    }
                }
            },
            "pwrite" => syn::parse_quote! {
                fn pwrite(
                    file: crate::builtin::GorsPtr<Self>,
                    b: crate::builtin::GorsSliceStorage<u8>,
                    off: i64,
                ) -> (isize, Box<dyn crate::builtin::error>) {
                    if off < 0 {
                        return (0, (*ErrInvalid).clone());
                    }
                    let resource = Self::__gors_resource(&file);
                    if !resource.writable {
                        return (0, Self::__gors_access_error());
                    }
                    let handle = match resource.handle.lock() {
                        Ok(handle) => handle,
                        Err(error) => return (0, Self::__gors_host_error(error)),
                    };
                    let __GorsHostFileHandle::Host(host) = &*handle else {
                        return (0, (*ErrClosed).clone());
                    };
                    #[cfg(unix)]
                    let result = {
                        use std::os::unix::fs::FileExt;
                        let bytes: &[u8] = b.as_ref();
                        host.write_at(bytes, off as u64)
                    };
                    #[cfg(windows)]
                    let result = {
                        use std::os::windows::fs::FileExt;
                        let bytes: &[u8] = b.as_ref();
                        host.seek_write(bytes, off as u64)
                    };
                    #[cfg(not(any(unix, windows)))]
                    let result: std::io::Result<usize> = Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "positional writes are unsupported on this host",
                    ));
                    match result {
                        Ok(0) if !b.is_empty() => (0, (*crate::io::ErrShortWrite).clone()),
                        Ok(written) => (written as isize, Self::__gors_nil_error()),
                        Err(error) => (0, Self::__gors_io_error(&error)),
                    }
                }
            },
            "seek" => syn::parse_quote! {
                fn seek(
                    file: crate::builtin::GorsPtr<Self>,
                    offset: i64,
                    whence: isize,
                ) -> (i64, Box<dyn crate::builtin::error>) {
                    let seek_from = match whence {
                        0 if offset >= 0 => std::io::SeekFrom::Start(offset as u64),
                        0 => return (0, (*ErrInvalid).clone()),
                        1 => std::io::SeekFrom::Current(offset),
                        2 => std::io::SeekFrom::End(offset),
                        _ => return (0, (*ErrInvalid).clone()),
                    };
                    let resource = Self::__gors_resource(&file);
                    let mut handle = match resource.handle.lock() {
                        Ok(handle) => handle,
                        Err(error) => return (0, Self::__gors_host_error(error)),
                    };
                    let __GorsHostFileHandle::Host(host) = &mut *handle else {
                        return (0, (*ErrClosed).clone());
                    };
                    match std::io::Seek::seek(host, seek_from) {
                        Ok(position) => (position as i64, Self::__gors_nil_error()),
                        Err(error) => (0, Self::__gors_io_error(&error)),
                    }
                }
            },
            "readFrom" => syn::parse_quote! {
                fn readFrom(
                    _file: crate::builtin::GorsPtr<Self>,
                    _reader: &mut dyn crate::io::Reader,
                ) -> (i64, bool, Box<dyn crate::builtin::error>) {
                    (0, false, Self::__gors_nil_error())
                }
            },
            "writeTo" => syn::parse_quote! {
                fn writeTo(
                    _file: crate::builtin::GorsPtr<Self>,
                    _writer: &mut dyn crate::io::Writer,
                ) -> (i64, bool, Box<dyn crate::builtin::error>) {
                    (0, false, Self::__gors_nil_error())
                }
            },
            "readdir" => syn::parse_quote! {
                fn readdir(
                    file: crate::builtin::GorsPtr<Self>,
                    n: isize,
                    mode: readdirMode,
                ) -> (
                    crate::builtin::GorsSliceStorage<String>,
                    crate::builtin::GorsSliceStorage<Box<dyn crate::io__fs::DirEntry>>,
                    crate::builtin::GorsSliceStorage<Box<dyn crate::io__fs::FileInfo>>,
                    Box<dyn crate::builtin::error>,
                ) {
                    let resource = Self::__gors_resource(&file);
                    if !resource.readable {
                        return (
                            Default::default(),
                            Default::default(),
                            Default::default(),
                            Self::__gors_access_error(),
                        );
                    }
                    {
                        let handle = match resource.handle.lock() {
                            Ok(handle) => handle,
                            Err(error) => {
                                return (
                                    Default::default(),
                                    Default::default(),
                                    Default::default(),
                                    Self::__gors_host_error(error),
                                );
                            }
                        };
                        if matches!(*handle, __GorsHostFileHandle::Closed) {
                            return (
                                Default::default(),
                                Default::default(),
                                Default::default(),
                                (*ErrClosed).clone(),
                            );
                        }
                    }
                    let mut directory = match resource.directory.lock() {
                        Ok(directory) => directory,
                        Err(error) => {
                            return (
                                Default::default(),
                                Default::default(),
                                Default::default(),
                                Self::__gors_host_error(error),
                            );
                        }
                    };
                    if directory.is_none() {
                        let path = Self::__gors_host_path(&resource.name);
                        let read_dir = match std::fs::read_dir(path) {
                            Ok(read_dir) => read_dir,
                            Err(error) => {
                                return (
                                    Default::default(),
                                    Default::default(),
                                    Default::default(),
                                    Self::__gors_path_error(
                                        "readdirent".to_string(),
                                        resource.name.clone(),
                                        Self::__gors_io_error(&error),
                                    ),
                                );
                            }
                        };
                        let mut entries = Vec::new();
                        for entry in read_dir {
                            match entry {
                                Ok(entry) => entries.push(entry.path()),
                                Err(error) => {
                                    return (
                                        Default::default(),
                                        Default::default(),
                                        Default::default(),
                                        Self::__gors_path_error(
                                            "readdirent".to_string(),
                                            resource.name.clone(),
                                            Self::__gors_io_error(&error),
                                        ),
                                    );
                                }
                            }
                        }
                        *directory = Some(__GorsHostDirState { entries, next: 0 });
                    }
                    let state = directory.as_mut().expect("initialized directory state");
                    let start = state.next;
                    let requested = usize::try_from(n).unwrap_or(0);
                    let end = if n <= 0 {
                        state.entries.len()
                    } else {
                        start.saturating_add(requested).min(state.entries.len())
                    };
                    let mut names = crate::builtin::GorsSliceStorage::default();
                    let mut dirents = crate::builtin::GorsSliceStorage::default();
                    let mut infos = crate::builtin::GorsSliceStorage::default();
                    for path in &state.entries[start..end] {
                        let name = path.file_name().map_or_else(
                            String::new,
                            |name| Self::__gors_go_path(std::path::Path::new(name)),
                        );
                        if mode.0 == 0 {
                            names.push_visible(name);
                            continue;
                        }
                        let metadata = match std::fs::symlink_metadata(path) {
                            Ok(metadata) => metadata,
                            Err(error) => {
                                return (
                                    names,
                                    dirents,
                                    infos,
                                    Self::__gors_path_error(
                                        "readdirent".to_string(),
                                        resource.name.clone(),
                                        Self::__gors_io_error(&error),
                                    ),
                                );
                            }
                        };
                        let type_mode = if metadata.file_type().is_dir() {
                            crate::io__fs::FileMode(1_u32 << 31)
                        } else if metadata.file_type().is_symlink() {
                            crate::io__fs::FileMode(1_u32 << 27)
                        } else {
                            crate::io__fs::FileMode(0)
                        };
                        let info = Self::__gors_file_info(name, &metadata);
                        if mode.0 == 1 {
                            dirents.push_visible(
                                Box::new(__GorsHostDirEntry { info, type_mode })
                                    as Box<dyn crate::io__fs::DirEntry>,
                            );
                        } else {
                            infos.push_visible(
                                Box::new(info) as Box<dyn crate::io__fs::FileInfo>,
                            );
                        }
                    }
                    state.next = end;
                    let error = if n > 0 && start == end {
                        (*crate::io::EOF).clone()
                    } else {
                        Self::__gors_nil_error()
                    };
                    (names, dirents, infos, error)
                }
            },
            _ => continue,
        };
        item_impl
            .items
            .push(syn::ImplItem::Fn(preserve_compiled_method_signature(
                original,
                replacement,
            )));
    }

    module.file.items.push(syn::Item::Impl(item_impl));
}

fn inject_host_private_file_methods(module: &mut CompiledModule, surface: &HostFileSurface) {
    let Some(original) = surface.private_file_method("close") else {
        return;
    };
    let replacement: syn::ImplItemFn = syn::parse_quote! {
        fn close(file: crate::builtin::GorsPtr<Self>) -> Box<dyn crate::builtin::error> {
            if file.is_nil() {
                return (*ErrInvalid).clone();
            }
            let resource = File::__gors_private_resource(&file);
            let path = match file.lock() {
                Ok(mut private) => {
                    private.pfd.Sysfd = -1;
                    private.name.clone()
                }
                Err(error) => return File::__gors_host_error(error),
            };
            let mut handle = match resource.handle.lock() {
                Ok(handle) => handle,
                Err(error) => return File::__gors_host_error(error),
            };
            if matches!(*handle, __GorsHostFileHandle::Closed) {
                return File::__gors_path_error(
                    "close".to_string(),
                    path,
                    (*ErrClosed).clone(),
                );
            }
            *handle = __GorsHostFileHandle::Closed;
            File::__gors_nil_error()
        }
    };
    let method = preserve_compiled_method_signature(original, replacement);
    module.file.items.push(syn::parse_quote! {
        #[allow(dead_code)]
        impl file {
            #method
        }
    });
}

fn inject_host_file_functions(module: &mut CompiledModule, surface: &HostFileSurface) {
    if surface.open_file_nolog && surface.has_host_file_boundary() {
        let required_flag = |name: &str, value: Option<isize>| {
            value.unwrap_or_else(|| panic!("missing pre-prune os flag metadata for {name}"))
        };
        let append_flag = required_flag("O_APPEND", surface.append_flag);
        let create_flag = required_flag("O_CREATE", surface.create_flag);
        let exclusive_flag = required_flag("O_EXCL", surface.exclusive_flag);
        let truncate_flag = required_flag("O_TRUNC", surface.truncate_flag);
        module.file.items.push(syn::parse_quote! {
            fn openFileNolog(
                name: String,
                flag: isize,
                perm: FileMode,
            ) -> (crate::builtin::GorsPtr<File>, Box<dyn crate::builtin::error>) {
                let append_mode = flag & #append_flag != 0;
                let create = flag & #create_flag != 0;
                let exclusive = flag & #exclusive_flag != 0;
                let truncate = flag & #truncate_flag != 0;
                let access_mode = flag & 3;
                if access_mode == 3 {
                    return (
                        Default::default(),
                        File::__gors_path_error(
                            "open".to_string(),
                            name,
                            (*ErrInvalid).clone(),
                        ),
                    );
                }
                let readable = access_mode != 1;
                let writable = access_mode != 0;
                let path = File::__gors_host_path(&name);
                let open_host = || -> std::io::Result<std::fs::File> {
                    #[cfg(unix)]
                    {
                        // Preserve the complete host flag word at the raw OS
                        // boundary. In particular, routing O_CREATE/O_TRUNC
                        // through OpenOptions' portable setters would require
                        // write access and produce a write-only descriptor for
                        // a Go O_RDONLY request. Supplying those bits as Unix
                        // custom flags keeps creation/truncation atomic while
                        // the standard read/write selectors retain the exact
                        // requested access mode. OpenOptions masks custom
                        // O_ACCMODE bits, so remove them explicitly as well.
                        let custom_flags = i32::try_from(flag & !3).map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "open flag does not fit the host ABI",
                            )
                        })?;
                        let mut options = std::fs::OpenOptions::new();
                        std::fs::OpenOptions::read(&mut options, readable);
                        std::fs::OpenOptions::write(&mut options, writable);
                        std::fs::OpenOptions::append(
                            &mut options,
                            append_mode && writable,
                        );
                        std::os::unix::fs::OpenOptionsExt::custom_flags(
                            &mut options,
                            custom_flags,
                        );
                        std::os::unix::fs::OpenOptionsExt::mode(&mut options, perm.0);
                        return std::fs::OpenOptions::open(&options, &path);
                    }

                    #[cfg(not(unix))]
                    {
                    // Rust OpenOptions requires write access for creation and
                    // truncation. Retain the exact handle that performed the
                    // atomic mutation and enforce the requested Go access mode
                    // in the raw methods; dropping and reopening here would
                    // introduce a path-replacement race.
                    if !writable && (create || truncate) {
                        if create && exclusive {
                            let mut creator = std::fs::OpenOptions::new();
                            std::fs::OpenOptions::write(&mut creator, true);
                            std::fs::OpenOptions::create_new(&mut creator, true);
                            #[cfg(unix)]
                            std::os::unix::fs::OpenOptionsExt::mode(&mut creator, perm.0);
                            return std::fs::OpenOptions::open(&creator, &path);
                        } else if truncate {
                            let mut modifier = std::fs::OpenOptions::new();
                            std::fs::OpenOptions::write(&mut modifier, true);
                            std::fs::OpenOptions::create(&mut modifier, create);
                            std::fs::OpenOptions::truncate(&mut modifier, true);
                            #[cfg(unix)]
                            std::os::unix::fs::OpenOptionsExt::mode(&mut modifier, perm.0);
                            return std::fs::OpenOptions::open(&modifier, &path);
                        } else {
                            let mut reader = std::fs::OpenOptions::new();
                            std::fs::OpenOptions::read(&mut reader, true);
                            match std::fs::OpenOptions::open(&reader, &path) {
                                Ok(existing) => return Ok(existing),
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    let mut creator = std::fs::OpenOptions::new();
                                    std::fs::OpenOptions::write(&mut creator, true);
                                    std::fs::OpenOptions::create_new(&mut creator, true);
                                    #[cfg(unix)]
                                    std::os::unix::fs::OpenOptionsExt::mode(
                                        &mut creator,
                                        perm.0,
                                    );
                                    match std::fs::OpenOptions::open(&creator, &path) {
                                        Ok(created) => return Ok(created),
                                        Err(race)
                                            if race.kind()
                                                == std::io::ErrorKind::AlreadyExists => {
                                                    return std::fs::OpenOptions::open(
                                                        &reader,
                                                        &path,
                                                    );
                                                }
                                        Err(error) => return Err(error),
                                    }
                                }
                                Err(error) => return Err(error),
                            }
                        }
                    }

                    let mut options = std::fs::OpenOptions::new();
                    std::fs::OpenOptions::read(&mut options, readable);
                    std::fs::OpenOptions::write(&mut options, writable);
                    std::fs::OpenOptions::append(&mut options, append_mode && writable);
                    if create && exclusive {
                        std::fs::OpenOptions::create_new(&mut options, true);
                    } else {
                        std::fs::OpenOptions::create(&mut options, create);
                        std::fs::OpenOptions::truncate(&mut options, truncate);
                    }
                    std::fs::OpenOptions::open(&options, &path)
                    }
                };
                match open_host() {
                    Ok(host) => (
                        crate::builtin::GorsPtr::new(File::__gors_from_host(
                            name,
                            host,
                            append_mode,
                            readable,
                            writable,
                        )),
                        File::__gors_nil_error(),
                    ),
                    Err(error) => (
                        Default::default(),
                        File::__gors_path_error(
                            "open".to_string(),
                            name,
                            File::__gors_io_error(&error),
                        ),
                    ),
                }
            }
        });
    }

    if surface.runtime_rand {
        module.file.items.push(syn::parse_quote! {
            fn runtime_rand() -> u64 {
                let counter = __GORS_HOST_RANDOM_COUNTER.fetch_add(
                    1,
                    std::sync::atomic::Ordering::Relaxed,
                );
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_nanos() as u64);
                nanos ^ counter.rotate_left(17) ^ u64::from(std::process::id())
            }
        });
    }
}
