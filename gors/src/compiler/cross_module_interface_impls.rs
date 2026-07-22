//! Canonical interface impls required across generated package boundaries.
//!
//! Go interface satisfaction is structural, so the concrete type's package
//! does not need to import the package that declares an interface. Rust still
//! needs one concrete trait impl. Reachability records those obligations as
//! `impl Trait for Concrete` roots on the concrete module; this pass turns only
//! those observed roots (plus embedded-supertrait obligations of existing
//! impls) into canonical impls owned by the concrete type's module.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use proc_macro2::Span;

use super::{
    CompiledModule, PackageFactMap, dce_iteration, dce_reachability,
    external_interface_implementors, generated_attrs, interface_impls, item_reachability,
    reachability_names, required_module_roots, syn_inspect, trait_impl_dedup, typeinfer,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TraitKey {
    module: String,
    name: String,
}

#[derive(Clone)]
struct TraitFact {
    go_name: String,
    rust_path: syn::Path,
    direct_methods: Vec<String>,
    embedded: Vec<TraitKey>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ImplTarget {
    Value,
    Pointer,
    BorrowedPointer,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Requirement {
    concrete_module: String,
    concrete_name: String,
    trait_key: TraitKey,
    target: ImplTarget,
}

#[derive(Clone)]
struct ConcreteFact {
    go_name: String,
    rust_type: syn::Type,
}

pub(super) fn inject(
    modules: &mut BTreeMap<String, CompiledModule>,
    program_type_envs: &PackageFactMap,
    has_main: bool,
) {
    let module_names = modules
        .values()
        .map(|module| module.mod_name.clone())
        .collect::<HashSet<_>>();
    let traits = trait_facts(modules, program_type_envs);
    if traits.is_empty() {
        return;
    }
    let concretes = concrete_facts(modules, program_type_envs);
    if concretes.is_empty() {
        return;
    }
    let exact_env = exact_type_env(modules, program_type_envs);
    let required = dce_iteration::discover_required_module_roots(modules, has_main);

    let mut requirements = requirements_from_existing_impls(
        modules,
        &module_names,
        &traits,
        &concretes,
        &required,
        has_main,
    );
    requirements.extend(requirements_from_external_roots(
        &required, &traits, &concretes,
    ));
    let requirements = expand_embedded_requirements(requirements, &traits);

    for requirement in requirements {
        let Some(concrete) = concretes.get(&(
            requirement.concrete_module.clone(),
            requirement.concrete_name.clone(),
        )) else {
            continue;
        };
        let Some(trait_fact) = traits.get(&requirement.trait_key) else {
            continue;
        };
        let Some(module) = modules
            .values_mut()
            .find(|module| module.mod_name == requirement.concrete_module)
        else {
            continue;
        };
        let methods = inherent_methods(&module.file.items);
        let pointer_methods = trait_fact
            .direct_methods
            .iter()
            .filter(|method| {
                exact_env.method_has_pointer_receiver(&format!("{}.{}", concrete.go_name, method))
            })
            .cloned()
            .collect::<BTreeSet<_>>();

        let value_satisfies = concrete_satisfies_trait(
            concrete,
            &requirement.trait_key,
            &traits,
            &exact_env,
            false,
            &mut BTreeSet::new(),
        );
        let pointer_satisfies = concrete_satisfies_trait(
            concrete,
            &requirement.trait_key,
            &traits,
            &exact_env,
            true,
            &mut BTreeSet::new(),
        );

        let target_satisfies = match requirement.target {
            ImplTarget::Value => value_satisfies,
            ImplTarget::Pointer | ImplTarget::BorrowedPointer => pointer_satisfies,
        };
        if !target_satisfies {
            continue;
        }
        let target = requirement.target;
        let Some(candidate) = impl_for_target(
            target,
            concrete,
            trait_fact,
            &requirement.concrete_name,
            &methods,
            &pointer_methods,
            &exact_env,
        ) else {
            continue;
        };
        insert_canonical_impl(module, candidate, &module_names);
    }
}

fn insert_canonical_impl(
    module: &mut CompiledModule,
    candidate: syn::Item,
    module_names: &HashSet<String>,
) -> bool {
    let target_matches = |item: &syn::Item| {
        trait_impl_dedup::impl_trait_targets_match_across_modules(
            item,
            &module.mod_name,
            &candidate,
            &module.mod_name,
            module_names,
        )
    };
    let canonical_exists = module
        .file
        .items
        .iter()
        .filter(|item| target_matches(item))
        .any(|item| {
            !matches!(item, syn::Item::Impl(item_impl)
                if generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs))
        });
    if canonical_exists {
        return false;
    }

    let module_name = module.mod_name.clone();
    module.file.items.retain(|item| {
        !matches!(item, syn::Item::Impl(item_impl)
        if generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs)
            && trait_impl_dedup::impl_trait_targets_match_across_modules(
                item,
                &module_name,
                &candidate,
                &module_name,
                module_names,
            ))
    });
    module.file.items.push(candidate);
    true
}

fn concrete_satisfies_trait(
    concrete: &ConcreteFact,
    trait_key: &TraitKey,
    traits: &BTreeMap<TraitKey, TraitFact>,
    env: &typeinfer::TypeEnv,
    include_pointer_receiver_methods: bool,
    visiting: &mut BTreeSet<TraitKey>,
) -> bool {
    if !visiting.insert(trait_key.clone()) {
        return true;
    }
    let Some(trait_fact) = traits.get(trait_key) else {
        visiting.remove(trait_key);
        return false;
    };
    let satisfies = trait_fact.direct_methods.iter().all(|method| {
        env.named_type_method_satisfies_interface_method(
            &concrete.go_name,
            &trait_fact.go_name,
            method,
            include_pointer_receiver_methods,
        )
    }) && trait_fact.embedded.iter().all(|embedded| {
        concrete_satisfies_trait(
            concrete,
            embedded,
            traits,
            env,
            include_pointer_receiver_methods,
            visiting,
        )
    });
    visiting.remove(trait_key);
    satisfies
}

fn trait_facts(
    modules: &BTreeMap<String, CompiledModule>,
    stdlib_type_envs: &PackageFactMap,
) -> BTreeMap<TraitKey, TraitFact> {
    let module_by_import_path = modules
        .values()
        .map(|module| (module.import_path.as_str(), module.mod_name.as_str()))
        .collect::<HashMap<_, _>>();
    let mut out = BTreeMap::new();

    for (import_path, facts) in stdlib_type_envs {
        let Some(module_name) = module_by_import_path.get(import_path.as_str()) else {
            continue;
        };
        for interface_name in facts
            .type_env()
            .interface_names()
            .into_iter()
            .filter(|name| !name.contains('.'))
        {
            let rust_interface_name = super::rust_safe_ident_name(&interface_name);
            let key = TraitKey {
                module: (*module_name).to_string(),
                name: rust_interface_name.clone(),
            };
            let module_ident = syn::Ident::new(module_name, Span::mixed_site());
            let trait_ident = syn::Ident::new(&rust_interface_name, Span::mixed_site());
            let embedded = facts
                .type_env()
                .get_interface_embedded_interfaces(&interface_name)
                .into_iter()
                .filter_map(|embedded| {
                    trait_key_from_go_name(
                        import_path,
                        &embedded,
                        stdlib_type_envs,
                        &module_by_import_path,
                    )
                })
                .collect();
            out.insert(
                key,
                TraitFact {
                    go_name: format!("{module_name}.{interface_name}"),
                    rust_path: syn::parse_quote! { crate::#module_ident::#trait_ident },
                    direct_methods: facts
                        .type_env()
                        .get_interface_direct_methods(&interface_name)
                        .unwrap_or_default(),
                    embedded,
                },
            );
        }
    }
    out
}

fn trait_key_from_go_name(
    current_import_path: &str,
    embedded: &str,
    stdlib_type_envs: &PackageFactMap,
    module_by_import_path: &HashMap<&str, &str>,
) -> Option<TraitKey> {
    let Some((package_name, interface_name)) = embedded.rsplit_once('.') else {
        let facts = stdlib_type_envs.get(current_import_path)?;
        if !facts.type_env().is_interface(embedded) {
            return None;
        }
        return Some(TraitKey {
            module: module_by_import_path.get(current_import_path)?.to_string(),
            name: super::rust_safe_ident_name(embedded),
        });
    };
    // Package-wide type facts are canonicalized to generated Rust module
    // identities before cross-module synthesis. Prefer that exact identity
    // when it is present; the Go package-name lookup below is only a fallback
    // for resolver facts that still carry source qualifiers.
    for (import_path, module_name) in module_by_import_path {
        if *module_name != package_name {
            continue;
        }
        let Some(facts) = stdlib_type_envs.get(*import_path) else {
            continue;
        };
        if facts.type_env().is_interface(interface_name) {
            return Some(TraitKey {
                module: (*module_name).to_string(),
                name: super::rust_safe_ident_name(interface_name),
            });
        }
    }
    let mut candidates = stdlib_type_envs.iter().filter(|(path, facts)| {
        path.as_str() != current_import_path
            && facts.package_name() == package_name
            && facts.type_env().is_interface(interface_name)
    });
    let (import_path, _) = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(TraitKey {
        module: module_by_import_path.get(import_path.as_str())?.to_string(),
        name: super::rust_safe_ident_name(interface_name),
    })
}

fn exact_type_env(
    modules: &BTreeMap<String, CompiledModule>,
    stdlib_type_envs: &PackageFactMap,
) -> typeinfer::TypeEnv {
    let module_by_import_path = modules
        .values()
        .map(|module| (module.import_path.as_str(), module.mod_name.as_str()))
        .collect::<HashMap<_, _>>();
    let package_name_counts =
        stdlib_type_envs
            .values()
            .fold(HashMap::<&str, usize>::new(), |mut counts, facts| {
                *counts.entry(facts.package_name()).or_default() += 1;
                counts
            });
    let mut env = typeinfer::TypeEnv::new();
    for (import_path, facts) in stdlib_type_envs {
        let Some(module_name) = module_by_import_path.get(import_path.as_str()) else {
            continue;
        };
        env.merge_package(module_name, facts.type_env());
        if *module_name != facts.package_name()
            && package_name_counts.get(facts.package_name()) == Some(&1)
        {
            // Scanned signatures retain imported Go package qualifiers. A
            // package-name alias is safe only when it identifies one import
            // path; generated module names remain the canonical identity.
            env.merge_package(facts.package_name(), facts.type_env());
        }
    }
    env
}

fn concrete_facts(
    modules: &BTreeMap<String, CompiledModule>,
    program_type_envs: &PackageFactMap,
) -> BTreeMap<(String, String), ConcreteFact> {
    let facts_by_import_path = program_type_envs
        .iter()
        .map(|(path, facts)| (path.as_str(), facts))
        .collect::<HashMap<_, _>>();
    let mut out = BTreeMap::new();
    for module in modules.values() {
        let Some(facts) = facts_by_import_path.get(module.import_path.as_str()) else {
            continue;
        };
        for item in &module.file.items {
            let syn::Item::Struct(item_struct) = item else {
                continue;
            };
            if !item_struct.generics.params.is_empty() {
                // Generic and lifetime-bearing generated storage has dedicated
                // interface emitters that own its impl generics and where
                // clauses. This canonical non-generic pass must not guess them.
                continue;
            }
            let name = item_struct.ident.to_string();
            // Defined non-struct Go types (for example `type NamedMap map[...]`)
            // are also lowered as tuple structs and can satisfy interfaces.
            // True aliases lower as Rust type aliases, so the generated item
            // shape plus a matching Go type fact identifies concrete owners.
            let Some(source_name) = source_type_name_for_rust_ident(facts.type_env(), &name) else {
                continue;
            };
            let ident = item_struct.ident.clone();
            out.insert(
                (module.mod_name.clone(), name.clone()),
                ConcreteFact {
                    go_name: format!("{}.{}", module.mod_name, source_name),
                    rust_type: syn::parse_quote! { #ident },
                },
            );
        }
    }
    out
}

fn source_type_name_for_rust_ident(env: &typeinfer::TypeEnv, rust_name: &str) -> Option<String> {
    if env.get_type_kind(rust_name).is_some() {
        return Some(rust_name.to_string());
    }
    let mut candidates = env
        .declared_type_names()
        .into_iter()
        .filter(|name| !name.contains('.') && super::rust_safe_ident_name(name) == rust_name);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn requirements_from_existing_impls(
    modules: &BTreeMap<String, CompiledModule>,
    module_names: &HashSet<String>,
    traits: &BTreeMap<TraitKey, TraitFact>,
    concretes: &BTreeMap<(String, String), ConcreteFact>,
    required: &required_module_roots::RequiredModuleRoots,
    has_main: bool,
) -> BTreeSet<Requirement> {
    let mut out = BTreeSet::new();
    for module in modules.values() {
        let main_roots;
        let roots = if module.is_main {
            main_roots = reachability_names::main_module_root_names(module, has_main);
            &main_roots
        } else {
            let Some(roots) = required.get(&module.mod_name) else {
                continue;
            };
            roots
        };
        if roots.is_empty() {
            continue;
        }
        let reachable =
            dce_reachability::reachable_stdlib_items(&module.file.items, roots, module_names);
        let item_names = reachability_names::item_reachability_names(&module.file.items);
        let top_level_names = reachability_names::top_level_item_names(&module.file.items);
        // A reachable coercion can be the first place a local concrete type is
        // required to satisfy an interface declared by another package. Such
        // an obligation has no existing impl item to inspect yet, so consume
        // the exact qualified impl roots collected from reachable bodies.
        for root in &reachable.names {
            let Some((trait_key, concrete_name, target)) = parse_qualified_impl_root(root) else {
                continue;
            };
            if !traits.contains_key(&trait_key)
                || !concretes.contains_key(&(module.mod_name.clone(), concrete_name.clone()))
            {
                continue;
            }
            out.insert(Requirement {
                concrete_module: module.mod_name.clone(),
                concrete_name,
                trait_key,
                target,
            });
        }
        for item in &module.file.items {
            if item_reachability::reachable_item_for_names(
                item,
                &reachable.names,
                &item_names,
                &top_level_names,
                roots,
            )
            .is_none()
            {
                continue;
            }
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            // Compiler-owned fallbacks are consumers of an exact interface
            // obligation, never evidence that the obligation is live. A dead
            // consumer can leave a preserve marker on its fallback; only the
            // qualified impl roots collected from reachable bodies above may
            // promote that fallback into a canonical owner impl.
            if generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs) {
                continue;
            }
            let Some((_, trait_path, _)) = &item_impl.trait_ else {
                continue;
            };
            let Some(trait_key) = trait_key_from_rust_path(trait_path, &module.mod_name, traits)
            else {
                continue;
            };
            let Some((concrete_module, concrete_name, target)) = concrete_impl_target(
                &item_impl.self_ty,
                &module.mod_name,
                module_names,
                concretes,
            ) else {
                continue;
            };
            out.insert(Requirement {
                concrete_module,
                concrete_name,
                trait_key,
                target,
            });
        }
    }
    out
}

fn requirements_from_external_roots(
    required: &required_module_roots::RequiredModuleRoots,
    traits: &BTreeMap<TraitKey, TraitFact>,
    concretes: &BTreeMap<(String, String), ConcreteFact>,
) -> BTreeSet<Requirement> {
    let mut out = BTreeSet::new();
    for (concrete_module, roots) in required.iter() {
        for root in roots {
            let Some((trait_key, concrete_name, target)) = parse_qualified_impl_root(root) else {
                continue;
            };
            if !traits.contains_key(&trait_key) {
                continue;
            }
            if !concretes.contains_key(&(concrete_module.clone(), concrete_name.clone())) {
                continue;
            }
            out.insert(Requirement {
                concrete_module: concrete_module.clone(),
                concrete_name: concrete_name.clone(),
                trait_key,
                target,
            });
        }
    }
    out
}

fn parse_qualified_impl_root(root: &str) -> Option<(TraitKey, String, ImplTarget)> {
    let body = root.strip_prefix("impl ")?;
    let (trait_path, concrete_name) = body.split_once(" for ")?;
    let (module, name) = trait_path.split_once("::")?;
    if module.is_empty() || name.is_empty() || name.contains("::") {
        return None;
    }
    let (concrete_name, target) = if let Some(name) = concrete_name
        .strip_prefix("GorsPtr<")
        .and_then(|name| name.strip_suffix('>'))
    {
        (name, ImplTarget::Pointer)
    } else if let Some(name) = concrete_name.strip_prefix("&mut ") {
        (name, ImplTarget::BorrowedPointer)
    } else {
        (concrete_name, ImplTarget::Value)
    };
    if concrete_name.is_empty() {
        return None;
    }
    Some((
        TraitKey {
            module: module.to_string(),
            name: name.to_string(),
        },
        concrete_name.to_string(),
        target,
    ))
}

fn expand_embedded_requirements(
    initial: BTreeSet<Requirement>,
    traits: &BTreeMap<TraitKey, TraitFact>,
) -> BTreeSet<Requirement> {
    let mut out = BTreeSet::new();
    let mut queue = initial.into_iter().collect::<VecDeque<_>>();
    while let Some(requirement) = queue.pop_front() {
        if !out.insert(requirement.clone()) {
            continue;
        }
        let Some(trait_fact) = traits.get(&requirement.trait_key) else {
            continue;
        };
        for embedded in &trait_fact.embedded {
            queue.push_back(Requirement {
                concrete_module: requirement.concrete_module.clone(),
                concrete_name: requirement.concrete_name.clone(),
                trait_key: embedded.clone(),
                target: requirement.target,
            });
        }
    }
    out
}

fn trait_key_from_rust_path(
    path: &syn::Path,
    current_module: &str,
    traits: &BTreeMap<TraitKey, TraitFact>,
) -> Option<TraitKey> {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    let name = segments.last()?.clone();
    let module = match segments.as_slice() {
        [crate_, module, ..] if crate_ == "crate" => module.clone(),
        [module, ..] if traits.keys().any(|key| key.module == *module) => module.clone(),
        [_] => current_module.to_string(),
        _ => return None,
    };
    let key = TraitKey { module, name };
    traits.contains_key(&key).then_some(key)
}

fn concrete_impl_target(
    ty: &syn::Type,
    current_module: &str,
    module_names: &HashSet<String>,
    concretes: &BTreeMap<(String, String), ConcreteFact>,
) -> Option<(String, String, ImplTarget)> {
    let (ty, target) = match ty {
        syn::Type::Reference(reference) if reference.mutability.is_some() => {
            (&*reference.elem, ImplTarget::BorrowedPointer)
        }
        _ => (ty, ImplTarget::Value),
    };
    let (ty, target) = gors_ptr_inner_type(ty)
        .map(|inner| (inner, ImplTarget::Pointer))
        .unwrap_or((ty, target));
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segments = path.path.segments.iter().collect::<Vec<_>>();
    let name = segments.last()?.ident.to_string();
    let module = match segments.as_slice() {
        [crate_, module, ..]
            if crate_.ident == "crate" && module_names.contains(&module.ident.to_string()) =>
        {
            module.ident.to_string()
        }
        [module, ..] if module_names.contains(&module.ident.to_string()) => {
            module.ident.to_string()
        }
        [_] => current_module.to_string(),
        _ => return None,
    };
    concretes
        .contains_key(&(module.clone(), name.clone()))
        .then_some((module, name, target))
}

fn gors_ptr_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "GorsPtr" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn inherent_methods(items: &[syn::Item]) -> BTreeMap<String, Vec<syn::ImplItemFn>> {
    let mut out = BTreeMap::<String, Vec<syn::ImplItemFn>>::new();
    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if item_impl.trait_.is_some() {
            continue;
        }
        let Some(type_name) = syn_inspect::named_self_type(&item_impl.self_ty) else {
            continue;
        };
        out.entry(type_name)
            .or_default()
            .extend(item_impl.items.iter().filter_map(|item| match item {
                syn::ImplItem::Fn(method) => Some(method.clone()),
                _ => None,
            }));
    }
    out
}

fn impl_for_target(
    target: ImplTarget,
    concrete: &ConcreteFact,
    trait_fact: &TraitFact,
    concrete_name: &str,
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    pointer_methods: &BTreeSet<String>,
    program_env: &typeinfer::TypeEnv,
) -> Option<syn::Item> {
    let trait_path = &trait_fact.rust_path;
    let concrete_ty = concrete.rust_type.clone();
    let (self_ty, impl_items): (syn::Type, Vec<syn::ImplItem>) = match target {
        ImplTarget::Value => {
            let record = external_interface_implementors::ExternalInterfaceImplementor {
                go_name: concrete.go_name.clone(),
                rust_ty: concrete_ty.clone(),
                include_pointer_receiver_methods: false,
                pointer_receiver_methods: BTreeSet::new(),
            };
            (
                concrete_ty,
                super::external_interface_impl_items(
                    &trait_fact.go_name,
                    trait_path,
                    &trait_fact.direct_methods,
                    &record,
                    program_env,
                    Some(methods),
                )?,
            )
        }
        ImplTarget::Pointer => {
            let self_ty: syn::Type = syn::parse_quote! { crate::builtin::GorsPtr<#concrete_ty> };
            let record = external_interface_implementors::ExternalInterfaceImplementor {
                go_name: concrete.go_name.clone(),
                rust_ty: self_ty.clone(),
                include_pointer_receiver_methods: true,
                pointer_receiver_methods: pointer_methods.clone(),
            };
            (
                self_ty,
                super::external_interface_impl_items(
                    &trait_fact.go_name,
                    trait_path,
                    &trait_fact.direct_methods,
                    &record,
                    program_env,
                    Some(methods),
                )?,
            )
        }
        ImplTarget::BorrowedPointer => {
            if !interface_impls::borrowed_pointer_can_delegate(
                &trait_fact.go_name,
                &trait_fact.direct_methods,
                Some(pointer_methods),
            ) {
                return None;
            }
            let lifetime = syn::Lifetime::new("'__gors", Span::mixed_site());
            let self_ty: syn::Type = syn::parse_quote! { &#lifetime mut #concrete_ty };
            (
                self_ty,
                interface_impls::borrowed_pointer_items_in_env(
                    &trait_fact.go_name,
                    concrete_name,
                    trait_path,
                    &trait_fact.direct_methods,
                    methods,
                    Some(pointer_methods),
                    program_env,
                ),
            )
        }
    };
    let mut attrs = Vec::new();
    generated_attrs::preserve_for_dce(&mut attrs);
    let generics = if target == ImplTarget::BorrowedPointer {
        let lifetime = syn::Lifetime::new("'__gors", Span::mixed_site());
        syn::parse_quote! { <#lifetime> }
    } else {
        syn::Generics::default()
    };
    Some(syn::Item::Impl(syn::ItemImpl {
        attrs,
        defaultness: None,
        unsafety: None,
        impl_token: Default::default(),
        generics,
        trait_: Some((None, trait_path.clone(), Default::default())),
        self_ty: Box::new(self_ty),
        brace_token: Default::default(),
        items: impl_items,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_pointer_impl_uses_explicit_program_interface_abi() {
        let _type_env_scope = super::super::LocalTypeEnvScopeGuard::push();
        super::super::set_type_env(typeinfer::TypeEnv::new());

        let mut program_env = typeinfer::TypeEnv::new();
        program_env.set_type_kind("fs.File", typeinfer::TypeKind::Interface);
        program_env.set_interface_methods("fs.File", vec!["Read".to_string()]);
        program_env.set_func_params(
            "fs.File.Read",
            vec![typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Uint8))],
        );
        program_env.set_func("fs.File.Read", vec![typeinfer::GoType::Int]);

        let methods = BTreeMap::from([(
            "File".to_string(),
            vec![syn::parse_quote! {
                pub fn Read(
                    mut file: crate::builtin::GorsPtr<Self>,
                    mut buffer: crate::builtin::GorsSliceStorage<u8>,
                ) -> isize {
                    0
                }
            }],
        )]);
        let concrete = ConcreteFact {
            go_name: "os.File".to_string(),
            rust_type: syn::parse_quote! { File },
        };
        let trait_fact = TraitFact {
            go_name: "io/fs.File".to_string(),
            rust_path: syn::parse_quote! { crate::io__fs::File },
            direct_methods: vec!["Read".to_string()],
            embedded: Vec::new(),
        };
        let item = impl_for_target(
            ImplTarget::BorrowedPointer,
            &concrete,
            &trait_fact,
            "File",
            &methods,
            &BTreeSet::from(["Read".to_string()]),
            &program_env,
        )
        .expect("borrowed pointer impl should be synthesized");
        let rendered = quote::quote! { #item }.to_string();

        assert!(
            rendered.contains("fn Read (& mut self , mut buffer : & mut [u8]) -> isize"),
            "the explicit whole-program interface environment must own the borrowed impl ABI: {rendered}"
        );
        assert!(
            !rendered.contains(
                "fn Read (& mut self , mut buffer : crate :: builtin :: GorsSliceStorage"
            ),
            "an insufficient thread-local environment must not leak the concrete ABI: {rendered}"
        );
    }

    fn test_module(mod_name: &str, import_path: &str) -> CompiledModule {
        CompiledModule {
            mod_name: mod_name.to_string(),
            import_path: import_path.to_string(),
            file: syn::parse_quote! {},
            filename: format!("{mod_name}.rs"),
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        }
    }

    #[test]
    fn qualified_impl_roots_preserve_exact_trait_module() {
        assert_eq!(
            parse_qualified_impl_root("impl first_io::Reader for File"),
            Some((
                TraitKey {
                    module: "first_io".to_string(),
                    name: "Reader".to_string(),
                },
                "File".to_string(),
                ImplTarget::Value,
            ))
        );
        assert_eq!(
            parse_qualified_impl_root("impl first_io::Reader for GorsPtr<File>"),
            Some((
                TraitKey {
                    module: "first_io".to_string(),
                    name: "Reader".to_string(),
                },
                "File".to_string(),
                ImplTarget::Pointer,
            ))
        );
        assert_eq!(parse_qualified_impl_root("impl Reader for File"), None);
    }

    #[test]
    fn duplicate_go_package_names_keep_distinct_generated_trait_identities() {
        let mut first = typeinfer::TypeEnv::new();
        first.set_interface_methods("Reader", vec!["Read".to_string()]);
        let mut second = typeinfer::TypeEnv::new();
        second.set_interface_methods("Reader", vec!["Read".to_string()]);
        let mut consumer = typeinfer::TypeEnv::new();
        consumer.set_interface_methods("Composite", Vec::new());
        consumer.set_interface_embedded("Composite", vec!["dup.Reader".to_string()]);

        let facts = PackageFactMap::from([
            (
                "example/first".to_string(),
                super::super::PackageFacts::new("dup".to_string(), first),
            ),
            (
                "example/second".to_string(),
                super::super::PackageFacts::new("dup".to_string(), second),
            ),
            (
                "example/consumer".to_string(),
                super::super::PackageFacts::new("consumer".to_string(), consumer),
            ),
        ]);
        let modules = BTreeMap::from([
            ("first".to_string(), test_module("first", "example/first")),
            (
                "second".to_string(),
                test_module("second", "example/second"),
            ),
            (
                "consumer".to_string(),
                test_module("consumer", "example/consumer"),
            ),
        ]);

        let traits = trait_facts(&modules, &facts);
        assert!(traits.contains_key(&TraitKey {
            module: "first".to_string(),
            name: "Reader".to_string(),
        }));
        assert!(traits.contains_key(&TraitKey {
            module: "second".to_string(),
            name: "Reader".to_string(),
        }));
        let module_by_import_path = modules
            .values()
            .map(|module| (module.import_path.as_str(), module.mod_name.as_str()))
            .collect::<HashMap<_, _>>();
        assert!(
            trait_key_from_go_name(
                "example/consumer",
                "first.Reader",
                &facts,
                &module_by_import_path,
            ) == Some(TraitKey {
                module: "first".to_string(),
                name: "Reader".to_string(),
            }),
            "the canonical generated module identity must win even when the Go package name is ambiguous"
        );
        assert!(
            trait_key_from_go_name(
                "example/consumer",
                "dup.Reader",
                &facts,
                &module_by_import_path,
            )
            .is_none(),
            "an ambiguous package-name fallback must not select either import path"
        );

        let env = exact_type_env(&modules, &facts);
        assert!(env.is_interface("first.Reader"));
        assert!(env.is_interface("second.Reader"));
        assert!(!env.is_interface("dup.Reader"));
    }

    #[test]
    fn rust_keyword_safe_item_names_keep_source_go_type_identity() {
        let mut iface_env = typeinfer::TypeEnv::new();
        iface_env.set_interface_methods("match", vec!["Read".to_string()]);
        let mut owner_env = typeinfer::TypeEnv::new();
        owner_env.set_type_kind("match", typeinfer::TypeKind::Struct);
        let facts = PackageFactMap::from([
            (
                "example/iface".to_string(),
                super::super::PackageFacts::new("iface".to_string(), iface_env),
            ),
            (
                "example/owner".to_string(),
                super::super::PackageFacts::new("owner".to_string(), owner_env),
            ),
        ]);
        let mut iface = test_module("iface", "example/iface");
        iface.file.items = vec![syn::parse_quote! {
            pub trait match_ {
                fn Read(&mut self);
            }
        }];
        let mut owner = test_module("owner", "example/owner");
        owner.file.items = vec![syn::parse_quote! {
            pub struct match_;
        }];
        let modules = BTreeMap::from([("iface".to_string(), iface), ("owner".to_string(), owner)]);

        let traits = trait_facts(&modules, &facts);
        let trait_key = TraitKey {
            module: "iface".to_string(),
            name: "match_".to_string(),
        };
        let trait_fact = traits.get(&trait_key);
        assert!(
            trait_fact.is_some(),
            "missing Rust-safe trait key {trait_key:?}; available keys: {:?}",
            traits.keys().collect::<Vec<_>>()
        );
        let Some(trait_fact) = trait_fact else {
            return;
        };
        assert_eq!(trait_fact.go_name, "iface.match");
        assert!(syn_inspect::path_is(
            &trait_fact.rust_path,
            &["crate", "iface", "match_"]
        ));

        let concretes = concrete_facts(&modules, &facts);
        let concrete_key = ("owner".to_string(), "match_".to_string());
        let concrete = concretes.get(&concrete_key);
        assert!(
            concrete.is_some(),
            "missing Rust-safe concrete key {concrete_key:?}; available keys: {:?}",
            concretes.keys().collect::<Vec<_>>()
        );
        let Some(concrete) = concrete else {
            return;
        };
        assert_eq!(concrete.go_name, "owner.match");
        assert_eq!(
            syn_inspect::named_self_type(&concrete.rust_type).as_deref(),
            Some("match_")
        );
    }

    #[test]
    fn dead_stdlib_consumer_fallback_does_not_create_owner_requirement() {
        let mut fallback: syn::ItemImpl = syn::parse_quote! {
            impl crate::iface::Reader for crate::owner::Source {}
        };
        generated_attrs::preserve_for_dce(&mut fallback.attrs);
        generated_attrs::mark_removable_interface_fallback(&mut fallback.attrs);
        let mut consumer = test_module("consumer", "consumer");
        consumer.file.items = vec![
            syn::parse_quote! { pub fn Live() {} },
            syn::Item::Impl(fallback),
        ];
        let modules = BTreeMap::from([
            ("consumer".to_string(), consumer),
            ("iface".to_string(), test_module("iface", "iface")),
            ("owner".to_string(), test_module("owner", "owner")),
        ]);
        let module_names = modules
            .values()
            .map(|module| module.mod_name.clone())
            .collect::<HashSet<_>>();
        let trait_key = TraitKey {
            module: "iface".to_string(),
            name: "Reader".to_string(),
        };
        let traits = BTreeMap::from([(
            trait_key,
            TraitFact {
                go_name: "iface.Reader".to_string(),
                rust_path: syn::parse_quote! { crate::iface::Reader },
                direct_methods: Vec::new(),
                embedded: Vec::new(),
            },
        )]);
        let concretes = BTreeMap::from([(
            ("owner".to_string(), "Source".to_string()),
            ConcreteFact {
                go_name: "owner.Source".to_string(),
                rust_type: syn::parse_quote! { Source },
            },
        )]);
        let mut required = required_module_roots::RequiredModuleRoots::default();
        required.merge(HashMap::from([(
            "consumer".to_string(),
            HashSet::from(["Live".to_string()]),
        )]));

        let requirements = requirements_from_existing_impls(
            &modules,
            &module_names,
            &traits,
            &concretes,
            &required,
            false,
        );

        assert!(requirements.is_empty(), "{requirements:?}");
    }

    #[test]
    fn same_basename_external_impls_synthesize_only_exact_requested_trait() {
        let mut owner = test_module("owner", "owner");
        owner.file.items = vec![
            syn::parse_quote! { pub struct Source; },
            syn::parse_quote! { impl crate::first_io::Reader for Source {} },
            syn::parse_quote! { impl crate::second_io::Reader for Source {} },
        ];
        let modules = BTreeMap::from([
            ("first_io".to_string(), test_module("first_io", "first_io")),
            ("owner".to_string(), owner),
            (
                "second_io".to_string(),
                test_module("second_io", "second_io"),
            ),
        ]);
        let module_names = modules
            .values()
            .map(|module| module.mod_name.clone())
            .collect::<HashSet<_>>();
        let traits = ["first_io", "second_io"]
            .into_iter()
            .map(|module| {
                let key = TraitKey {
                    module: module.to_string(),
                    name: "Reader".to_string(),
                };
                let module_ident = syn::Ident::new(module, Span::mixed_site());
                (
                    key,
                    TraitFact {
                        go_name: format!("{module}.Reader"),
                        rust_path: syn::parse_quote! { crate::#module_ident::Reader },
                        direct_methods: Vec::new(),
                        embedded: Vec::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let concretes = BTreeMap::from([(
            ("owner".to_string(), "Source".to_string()),
            ConcreteFact {
                go_name: "owner.Source".to_string(),
                rust_type: syn::parse_quote! { Source },
            },
        )]);
        let mut required = required_module_roots::RequiredModuleRoots::default();
        required.merge(HashMap::from([(
            "owner".to_string(),
            HashSet::from([
                "Source".to_string(),
                item_reachability::trait_impl_reachability_name("Reader", "Source"),
                item_reachability::qualified_trait_impl_reachability_name(
                    "first_io", "Reader", "Source",
                ),
            ]),
        )]));

        let requirements = requirements_from_existing_impls(
            &modules,
            &module_names,
            &traits,
            &concretes,
            &required,
            false,
        );

        assert_eq!(requirements.len(), 1, "{requirements:?}");
        assert!(requirements.contains(&Requirement {
            concrete_module: "owner".to_string(),
            concrete_name: "Source".to_string(),
            trait_key: TraitKey {
                module: "first_io".to_string(),
                name: "Reader".to_string(),
            },
            target: ImplTarget::Value,
        }));
    }

    #[test]
    fn canonical_owner_impl_replaces_matching_removable_fallback() {
        let mut module = test_module("os", "os");
        module.file = syn::parse_quote! {
            pub struct File;

            #[doc = "gors:removable-interface-fallback"]
            impl io::Reader for File {}
        };
        let mut attrs = Vec::new();
        generated_attrs::preserve_for_dce(&mut attrs);
        let candidate = syn::Item::Impl(syn::ItemImpl {
            attrs,
            defaultness: None,
            unsafety: None,
            impl_token: Default::default(),
            generics: syn::Generics::default(),
            trait_: Some((
                None,
                syn::parse_quote! { crate::io::Reader },
                Default::default(),
            )),
            self_ty: Box::new(syn::parse_quote! { File }),
            brace_token: Default::default(),
            items: Vec::new(),
        });
        let module_names = HashSet::from(["io".to_string(), "os".to_string()]);

        assert!(insert_canonical_impl(&mut module, candidate, &module_names));
        let matching = module
            .file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl) if item_impl.trait_.is_some() => Some(item_impl),
                _ => None,
            })
            .collect::<Vec<_>>();
        match matching.as_slice() {
            [matching] => {
                assert!(generated_attrs::attrs_preserve_for_dce(&matching.attrs));
                assert!(!generated_attrs::attrs_mark_removable_interface_fallback(
                    &matching.attrs
                ));
            }
            _ => assert_eq!(
                matching.len(),
                1,
                "expected exactly one canonical matching impl"
            ),
        }
    }
}
