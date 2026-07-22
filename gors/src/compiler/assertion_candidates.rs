//! Candidate management for interface-to-interface assertions.
//!
//! Go's structural interface model makes every program concrete type a
//! potential dynamic assertion result. Emitting that conservative census into
//! Rust is necessary while resolving packages, but those candidate branches
//! must not make their own concrete types reachable. This module marks each
//! branch, removes the census from a candidate-blind DCE snapshot, and then
//! retains only candidates whose concrete type survived for an ordinary
//! program reason.

use std::collections::{BTreeMap, HashSet};

use syn::visit_mut::{self, VisitMut};

use super::{CompiledModule, generated_attrs};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ConcreteTypeKey {
    module: String,
    name: String,
}

pub(super) type LiveConcreteTypes = HashSet<ConcreteTypeKey>;

pub(super) fn mark(concrete: &syn::Type, candidate: syn::Expr) -> syn::Expr {
    let mut attrs = Vec::new();
    generated_attrs::mark_interface_assertion_candidate(&mut attrs, concrete);
    syn::Expr::Block(syn::ExprBlock {
        attrs,
        label: None,
        block: syn::parse_quote!({ #candidate }),
    })
}

/// Remove all assertion-census branches and their compiler-owned fallback
/// impls before computing ordinary reachability.
pub(super) fn strip_for_reachability(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut() {
        module.file.items.retain(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return true;
            };
            !(generated_attrs::attrs_mark_external_local_interface_impl(&item_impl.attrs)
                || generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs))
        });
        CandidateBranchPruner {
            current_module: &module.mod_name,
            module_names: &HashSet::new(),
            live: None,
        }
        .visit_file_mut(&mut module.file);
    }
}

pub(super) fn live_concrete_types(modules: &BTreeMap<String, CompiledModule>) -> LiveConcreteTypes {
    modules
        .values()
        .flat_map(|module| {
            module.file.items.iter().filter_map(|item| {
                let syn::Item::Struct(item_struct) = item else {
                    return None;
                };
                Some(ConcreteTypeKey {
                    module: module.mod_name.clone(),
                    name: item_struct.ident.to_string(),
                })
            })
        })
        .collect()
}

/// Remove dead candidates from the real program and unwrap the compiler-only
/// marker around every retained branch. Unknown type spellings are retained
/// conservatively.
pub(super) fn retain_live(
    modules: &mut BTreeMap<String, CompiledModule>,
    live: &LiveConcreteTypes,
) {
    let module_names = modules
        .values()
        .map(|module| module.mod_name.clone())
        .collect::<HashSet<_>>();
    for module in modules.values_mut() {
        let current_module = module.mod_name.clone();
        module.file.items.retain(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return true;
            };
            if !(generated_attrs::attrs_mark_external_local_interface_impl(&item_impl.attrs)
                || generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs))
            {
                return true;
            }
            candidate_key(&item_impl.self_ty, &current_module, &module_names)
                .is_none_or(|key| live.contains(&key))
        });
        CandidateBranchPruner {
            current_module: &current_module,
            module_names: &module_names,
            live: Some(live),
        }
        .visit_file_mut(&mut module.file);
    }
}

struct CandidateBranchPruner<'a> {
    current_module: &'a str,
    module_names: &'a HashSet<String>,
    /// `None` is the candidate-blind snapshot; `Some` filters the real tree.
    live: Option<&'a LiveConcreteTypes>,
}

impl CandidateBranchPruner<'_> {
    fn should_keep(&self, concrete: &syn::Type) -> bool {
        let Some(live) = self.live else {
            return false;
        };
        candidate_key(concrete, self.current_module, self.module_names)
            .is_none_or(|key| live.contains(&key))
    }
}

impl VisitMut for CandidateBranchPruner<'_> {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        let Some((concrete, mut candidate)) = take_marked_candidate(expr) else {
            visit_mut::visit_expr_mut(self, expr);
            return;
        };

        if self.should_keep(&concrete) {
            *expr = candidate;
        } else if let Some(fallback) = take_candidate_fallback(&mut candidate) {
            *expr = fallback;
        } else if self.live.is_some() {
            // A malformed compiler marker must never make the real program
            // less capable. Keep its body, while still removing the marker.
            debug_assert!(false, "interface assertion candidate lost its fallback");
            *expr = candidate;
        } else {
            // The snapshot is analysis-only. If an internal transform changed
            // the marked shape, erase it rather than letting the census root
            // itself and surface the debug assertion in development builds.
            debug_assert!(false, "interface assertion candidate lost its fallback");
            *expr = syn::parse_quote! { unreachable!() };
        }
        self.visit_expr_mut(expr);
    }
}

fn take_marked_candidate(expr: &mut syn::Expr) -> Option<(syn::Type, syn::Expr)> {
    let syn::Expr::Block(block) = expr else {
        return None;
    };
    let concrete = generated_attrs::interface_assertion_candidate_type(&block.attrs)?;
    if block.block.stmts.len() != 1 {
        return None;
    }
    let stmt = block.block.stmts.pop()?;
    let syn::Stmt::Expr(candidate, None) = stmt else {
        return None;
    };
    Some((concrete, candidate))
}

fn take_candidate_fallback(candidate: &mut syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::If(candidate) = candidate else {
        return None;
    };
    candidate
        .else_branch
        .take()
        .map(|(_else_token, fallback)| *fallback)
}

fn candidate_key(
    concrete: &syn::Type,
    current_module: &str,
    module_names: &HashSet<String>,
) -> Option<ConcreteTypeKey> {
    let concrete = candidate_base_type(concrete)?;
    let syn::Type::Path(concrete) = concrete else {
        return None;
    };
    if concrete.qself.is_some() {
        return None;
    }
    let segments = concrete
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [krate, module, name] if krate == "crate" && module_names.contains(module) => {
            Some(ConcreteTypeKey {
                module: module.clone(),
                name: name.clone(),
            })
        }
        [krate, name] if krate == "crate" => Some(ConcreteTypeKey {
            module: "main".to_string(),
            name: name.clone(),
        }),
        [module, name] if module_names.contains(module) => Some(ConcreteTypeKey {
            module: module.clone(),
            name: name.clone(),
        }),
        [name] => Some(ConcreteTypeKey {
            module: current_module.to_string(),
            name: name.clone(),
        }),
        _ => None,
    }
}

fn candidate_base_type(ty: &syn::Type) -> Option<&syn::Type> {
    match ty {
        syn::Type::Group(group) => candidate_base_type(&group.elem),
        syn::Type::Paren(paren) => candidate_base_type(&paren.elem),
        syn::Type::Ptr(ptr) => candidate_base_type(&ptr.elem),
        syn::Type::Reference(reference) => candidate_base_type(&reference.elem),
        syn::Type::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "GorsPtr") =>
        {
            let syn::PathArguments::AngleBracketed(arguments) =
                &path.path.segments.last()?.arguments
            else {
                return None;
            };
            arguments.args.iter().find_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => candidate_base_type(ty),
                _ => None,
            })
        }
        _ => Some(ty),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_filter_drops_dead_pointer_branch_and_unwraps_live_branch() {
        let dead_ty: syn::Type = syn::parse_quote! { crate::builtin::GorsPtr<crate::model::Dead> };
        let live_ty: syn::Type = syn::parse_quote! { crate::builtin::GorsPtr<crate::model::Live> };
        let fallback: syn::Expr = syn::parse_quote! { fallback() };
        let dead_candidate: syn::Expr = syn::parse_quote! {
            if is_type::<#dead_ty>() { dead() } else { #fallback }
        };
        let dead = mark(&dead_ty, dead_candidate);
        let live_candidate: syn::Expr = syn::parse_quote! {
            if is_type::<#live_ty>() { live() } else { #dead }
        };
        let mut expr = mark(&live_ty, live_candidate);
        let live = HashSet::from([ConcreteTypeKey {
            module: "model".to_string(),
            name: "Live".to_string(),
        }]);
        CandidateBranchPruner {
            current_module: "iface",
            module_names: &HashSet::from(["model".to_string()]),
            live: Some(&live),
        }
        .visit_expr_mut(&mut expr);

        let rendered = quote::quote! { #expr }.to_string();
        assert!(rendered.contains("Live"), "{rendered}");
        assert!(!rendered.contains("Dead"), "{rendered}");
        assert!(
            !rendered.contains(crate::generated_names::INTERFACE_ASSERTION_CANDIDATE_DOC_PREFIX),
            "{rendered}"
        );
    }

    #[test]
    fn candidate_blind_filter_removes_every_marked_branch() {
        let concrete: syn::Type = syn::parse_quote! { crate::model::Dead };
        let candidate: syn::Expr = syn::parse_quote! {
            if is_type::<#concrete>() { dead() } else { fallback() }
        };
        let mut expr = mark(&concrete, candidate);
        CandidateBranchPruner {
            current_module: "iface",
            module_names: &HashSet::new(),
            live: None,
        }
        .visit_expr_mut(&mut expr);

        assert_eq!(quote::quote! { #expr }.to_string(), "{ fallback () }");
    }
}
