use super::syn_helpers::{ImplSelfType, has_impl, has_struct, has_trait, trait_methods};
use crate::generated_names::{
    ERROR_EXT_TRAIT, NOOP_INTERFACE, error_ext_trait_ident, noop_interface_ident,
};
use crate::noop_methods::{CloneBoxPolicy, MethodPolicy, NonHookReturnPolicy};

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    let facts = NoopInterfaceFacts::collect(items);

    if !facts.has_noop_uses() {
        return;
    }

    if !has_struct(items, NOOP_INTERFACE) {
        items.insert(0, noop_struct_item());
    }

    for target in facts.required_trait_targets() {
        if has_impl(items, &target, ImplSelfType::Named(NOOP_INTERFACE)) {
            continue;
        }
        let Some(methods) = trait_methods(items, &target) else {
            continue;
        };
        items.insert(0, noop_trait_impl(&target, &methods));
    }

    inject_error_ext(items);
}

fn noop_struct_item() -> syn::Item {
    let noop_ident = noop_interface_ident();
    syn::parse_quote! {
        #[derive(Clone, Default)]
        struct #noop_ident;
    }
}

fn noop_trait_impl(trait_name: &str, methods: &[syn::TraitItemFn]) -> syn::Item {
    let trait_ident = syn::Ident::new(trait_name, proc_macro2::Span::mixed_site());
    let noop_ident = noop_interface_ident();
    let default_expr: syn::Expr = syn::parse_quote! { Self::default() };
    let trait_path: syn::Path = syn::parse_quote! { #trait_ident };
    let policy = MethodPolicy {
        clone_box: CloneBoxPolicy::BoxDefault {
            default_expr: &default_expr,
            trait_path: &trait_path,
        },
        non_hook_return: NonHookReturnPolicy::Default,
    };
    let impl_methods = methods
        .iter()
        .map(|method| crate::noop_methods::impl_fn_for_trait_method(method, &policy))
        .collect::<Vec<_>>();

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #(#impl_methods)*
        }
    }
}

fn inject_error_ext(items: &mut Vec<syn::Item>) {
    let trait_ident = error_ext_trait_ident();
    let noop_ident = noop_interface_ident();

    if !has_trait(items, ERROR_EXT_TRAIT) {
        items.insert(
            0,
            syn::parse_quote! {
                trait #trait_ident {
                    fn Error(&mut self) -> String;
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT_TRAIT, ImplSelfType::Named("String")) {
        items.insert(
            0,
            syn::parse_quote! {
                impl #trait_ident for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT_TRAIT, ImplSelfType::Named(NOOP_INTERFACE)) {
        items.insert(0, noop_error_ext_impl(&trait_ident, &noop_ident));
    }
}

fn noop_error_ext_impl(trait_ident: &syn::Ident, noop_ident: &syn::Ident) -> syn::Item {
    let method: syn::TraitItemFn = syn::parse_quote! {
        fn Error(&mut self) -> String;
    };
    let default_expr: syn::Expr = syn::parse_quote! { Self::default() };
    let trait_path: syn::Path = syn::parse_quote! { #trait_ident };
    let policy = MethodPolicy {
        clone_box: CloneBoxPolicy::BoxDefault {
            default_expr: &default_expr,
            trait_path: &trait_path,
        },
        non_hook_return: NonHookReturnPolicy::Default,
    };
    let impl_method = crate::noop_methods::impl_fn_for_trait_method(&method, &policy);

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #impl_method
        }
    }
}

struct NoopInterfaceFacts {
    traits: std::collections::BTreeSet<String>,
    item_names: std::collections::BTreeSet<String>,
    noop_method_uses: Vec<std::collections::BTreeSet<String>>,
    trait_methods: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    signature_dependencies: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl NoopInterfaceFacts {
    fn collect(items: &[syn::Item]) -> Self {
        let mut traits = std::collections::BTreeSet::new();
        let mut item_names = std::collections::BTreeSet::new();
        let mut trait_methods_by_name = std::collections::BTreeMap::new();
        let mut signature_dependencies = std::collections::BTreeMap::new();
        let mut noop_uses = NoopUseCollector::default();

        for item in items {
            if let Some(name) = item_name(item) {
                item_names.insert(name);
            }
            syn::visit::Visit::visit_item(&mut noop_uses, item);
            let syn::Item::Trait(item_trait) = item else {
                continue;
            };
            let name = item_trait.ident.to_string();
            traits.insert(name.clone());
            trait_methods_by_name.insert(name.clone(), trait_method_names(item_trait));
            signature_dependencies.insert(name, trait_signature_dependencies(item_trait));
        }

        Self {
            traits,
            item_names,
            noop_method_uses: noop_uses.finish(),
            trait_methods: trait_methods_by_name,
            signature_dependencies,
        }
    }

    fn has_noop_uses(&self) -> bool {
        !self.noop_method_uses.is_empty()
    }

    fn required_trait_targets(&self) -> std::collections::BTreeSet<String> {
        let mut targets = std::collections::BTreeSet::new();
        for methods in &self.noop_method_uses {
            let called_trait_methods = methods
                .iter()
                .filter(|method| method.as_str() != "Error")
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if called_trait_methods.is_empty() {
                continue;
            }
            let matching_traits = self
                .traits
                .iter()
                .filter(|trait_name| self.should_inject(trait_name, &called_trait_methods))
                .cloned()
                .collect::<Vec<_>>();
            if let [target] = matching_traits.as_slice() {
                targets.insert(target.clone());
            }
        }
        targets
    }

    fn should_inject(
        &self,
        trait_name: &str,
        called_trait_methods: &std::collections::BTreeSet<String>,
    ) -> bool {
        let Some(methods) = self.trait_methods.get(trait_name) else {
            return false;
        };
        called_trait_methods
            .iter()
            .all(|method| methods.contains(method))
            && self
                .signature_dependencies
                .get(trait_name)
                .is_none_or(|dependencies| {
                    dependencies
                        .iter()
                        .all(|dependency| self.item_names.contains(dependency))
                })
    }
}

#[derive(Default)]
struct NoopUseCollector {
    scopes: Vec<std::collections::BTreeMap<String, Option<usize>>>,
    methods_by_binding: Vec<std::collections::BTreeSet<String>>,
}

impl NoopUseCollector {
    fn finish(self) -> Vec<std::collections::BTreeSet<String>> {
        self.methods_by_binding
    }

    fn push_scope(&mut self) {
        self.scopes.push(std::collections::BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind_name(&mut self, name: String, is_noop: bool) {
        if self.scopes.is_empty() {
            self.push_scope();
        }
        let binding = if is_noop {
            let index = self.methods_by_binding.len();
            self.methods_by_binding
                .push(std::collections::BTreeSet::new());
            Some(index)
        } else {
            None
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, binding);
        }
    }

    fn binding_for_name(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .flatten()
    }

    fn bind_pat_with_noop_expr(&mut self, pat: &syn::Pat, expr: Option<&syn::Expr>) {
        match (pat, expr) {
            (syn::Pat::Tuple(tuple_pat), Some(syn::Expr::Tuple(tuple_expr))) => {
                for (pat, expr) in tuple_pat.elems.iter().zip(tuple_expr.elems.iter()) {
                    self.bind_pat_with_noop_expr(pat, Some(expr));
                }
                for pat in tuple_pat.elems.iter().skip(tuple_expr.elems.len()) {
                    self.bind_pat_with_noop_expr(pat, None);
                }
            }
            (syn::Pat::Paren(paren), expr) => self.bind_pat_with_noop_expr(&paren.pat, expr),
            (syn::Pat::Ident(ident), expr) => {
                self.bind_name(
                    ident.ident.to_string(),
                    expr.is_some_and(expr_is_noop_default),
                );
            }
            _ => {
                for ident in pat_idents(pat) {
                    self.bind_name(ident, false);
                }
            }
        }
    }

    fn bind_fn_args(&mut self, inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>) {
        for input in inputs {
            let syn::FnArg::Typed(typed) = input else {
                continue;
            };
            for ident in pat_idents(&typed.pat) {
                self.bind_name(ident, false);
            }
        }
    }
}

impl syn::visit::Visit<'_> for NoopUseCollector {
    fn visit_item_fn(&mut self, func: &syn::ItemFn) {
        self.push_scope();
        self.bind_fn_args(&func.sig.inputs);
        syn::visit::visit_block(self, &func.block);
        self.pop_scope();
    }

    fn visit_impl_item_fn(&mut self, func: &syn::ImplItemFn) {
        self.push_scope();
        self.bind_fn_args(&func.sig.inputs);
        syn::visit::visit_block(self, &func.block);
        self.pop_scope();
    }

    fn visit_expr_closure(&mut self, closure: &syn::ExprClosure) {
        self.push_scope();
        for input in &closure.inputs {
            for ident in pat_idents(input) {
                self.bind_name(ident, false);
            }
        }
        syn::visit::visit_expr(self, &closure.body);
        self.pop_scope();
    }

    fn visit_block(&mut self, block: &syn::Block) {
        self.push_scope();
        for stmt in &block.stmts {
            syn::visit::Visit::visit_stmt(self, stmt);
        }
        self.pop_scope();
    }

    fn visit_local(&mut self, local: &syn::Local) {
        syn::visit::visit_local(self, local);
        self.bind_pat_with_noop_expr(
            &local.pat,
            local.init.as_ref().map(|init| init.expr.as_ref()),
        );
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if let Some(name) = expr_path_ident(&call.receiver)
            && let Some(binding) = self.binding_for_name(&name)
            && let Some(methods) = self.methods_by_binding.get_mut(binding)
        {
            methods.insert(call.method.to_string());
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn pat_idents(pat: &syn::Pat) -> Vec<String> {
    struct Finder {
        idents: Vec<String>,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_pat_ident(&mut self, pat: &syn::PatIdent) {
            self.idents.push(pat.ident.to_string());
        }
    }

    let mut finder = Finder { idents: Vec::new() };
    syn::visit::Visit::visit_pat(&mut finder, pat);
    finder.idents
}

fn expr_path_ident(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Group(group) => expr_path_ident(&group.expr),
        syn::Expr::Paren(paren) => expr_path_ident(&paren.expr),
        syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => path
            .path
            .segments
            .first()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn expr_is_noop_default(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Group(group) => expr_is_noop_default(&group.expr),
        syn::Expr::Paren(paren) => expr_is_noop_default(&paren.expr),
        syn::Expr::Call(call) => {
            let syn::Expr::Path(path) = call.func.as_ref() else {
                return false;
            };
            path.qself.is_none()
                && path.path.leading_colon.is_none()
                && path.path.segments.len() == 2
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == NOOP_INTERFACE)
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "default")
                && call.args.is_empty()
        }
        _ => false,
    }
}

fn trait_method_names(item_trait: &syn::ItemTrait) -> std::collections::BTreeSet<String> {
    item_trait
        .items
        .iter()
        .filter_map(|item| {
            let syn::TraitItem::Fn(func) = item else {
                return None;
            };
            let name = func.sig.ident.to_string();
            (!name.starts_with("__gors_")).then_some(name)
        })
        .collect()
}

fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item_const) => Some(item_const.ident.to_string()),
        syn::Item::Enum(item_enum) => Some(item_enum.ident.to_string()),
        syn::Item::Fn(item_fn) => Some(item_fn.sig.ident.to_string()),
        syn::Item::Static(item_static) => Some(item_static.ident.to_string()),
        syn::Item::Struct(item_struct) => Some(item_struct.ident.to_string()),
        syn::Item::Trait(item_trait) => Some(item_trait.ident.to_string()),
        syn::Item::Type(item_type) => Some(item_type.ident.to_string()),
        _ => None,
    }
}

fn trait_signature_dependencies(item_trait: &syn::ItemTrait) -> std::collections::BTreeSet<String> {
    struct Finder {
        dependencies: std::collections::BTreeSet<String>,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_type_param_bound(&mut self, bound: &syn::TypeParamBound) {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                collect_signature_dependency_path(&trait_bound.path, &mut self.dependencies);
            }
            syn::visit::visit_type_param_bound(self, bound);
        }

        fn visit_type_path(&mut self, type_path: &syn::TypePath) {
            collect_signature_dependency_path(&type_path.path, &mut self.dependencies);
            syn::visit::visit_type_path(self, type_path);
        }
    }

    let mut finder = Finder {
        dependencies: std::collections::BTreeSet::new(),
    };
    for item in &item_trait.items {
        let syn::TraitItem::Fn(func) = item else {
            continue;
        };
        syn::visit::Visit::visit_signature(&mut finder, &func.sig);
    }
    finder.dependencies
}

fn collect_signature_dependency_path(
    path: &syn::Path,
    dependencies: &mut std::collections::BTreeSet<String>,
) {
    if path.leading_colon.is_some() || path.segments.len() != 1 {
        return;
    }
    let Some(name) = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
    else {
        return;
    };
    if is_rust_signature_builtin(&name) {
        return;
    }
    dependencies.insert(name);
}

fn is_rust_signature_builtin(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "Box"
            | "Option"
            | "Result"
            | "Self"
            | "String"
            | "Vec"
            | "bool"
            | "char"
            | "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
    )
}
