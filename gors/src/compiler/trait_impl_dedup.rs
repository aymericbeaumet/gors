use super::{CompiledModule, generated_attrs, syn_inspect};
use proc_macro2::TokenTree;
use quote::ToTokens;
use std::collections::{BTreeMap, HashSet};
use syn::visit_mut::{self, VisitMut};

/// Remove only trait impls whose complete emitted syntax is structurally
/// equivalent to an earlier impl for the same trait and self type.
///
/// Go packages are lowered one source file at a time. An interface with only
/// embedded methods can therefore cause multiple files to synthesize the same
/// empty/direct-method impl. Rust coherence is package-wide, so those exact
/// duplicates must be collapsed after the package items are assembled. A
/// divergent impl for the same target is deliberately retained so rustc still
/// diagnoses a real compiler disagreement instead of this pass choosing one.
pub fn dedupe_equivalent_trait_impls(items: &mut Vec<syn::Item>) {
    let mut deduped = Vec::with_capacity(items.len());
    let mut trait_impl_indices = Vec::new();

    for item in std::mem::take(items) {
        let duplicate = matches!(&item, syn::Item::Impl(item_impl) if item_impl.trait_.is_some())
            && trait_impl_indices.iter().copied().any(|index| {
                deduped.get(index).is_some_and(|existing| {
                    syn_inspect::impl_trait_targets_match(existing, &item)
                        && items_are_structurally_equivalent(existing, &item)
                })
            });
        if duplicate {
            continue;
        }
        if matches!(&item, syn::Item::Impl(item_impl) if item_impl.trait_.is_some()) {
            trait_impl_indices.push(deduped.len());
        }
        deduped.push(item);
    }

    *items = deduped;
}

/// Apply whole-program trait-impl ownership after DCE has selected the final
/// module items.
///
/// Consumer modules sometimes synthesize a compiler-marked fallback impl for a
/// concrete type defined in another generated module. If that defining module
/// already contains the canonical impl, retaining the fallback violates Rust's
/// coherence rule. Only compiler-marked imported or external-local fallbacks
/// are removed; arbitrary same-target impls and differing owner impls remain
/// visible to rustc.
pub fn dedupe_program_trait_impls(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut() {
        dedupe_equivalent_trait_impls(&mut module.file.items);
    }

    let module_names = modules
        .values()
        .map(|module| module.mod_name.clone())
        .collect::<HashSet<_>>();
    let canonical_owner_targets = modules
        .values()
        .flat_map(|module| {
            module.file.items.iter().filter_map(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return None;
                };
                if item_impl.trait_.is_none() || impl_is_compiler_fallback(item_impl) {
                    return None;
                }
                let target =
                    canonical_trait_impl_target(item_impl, &module.mod_name, &module_names)?;
                (self_type_owner_module(&target.self_ty, &module_names).as_deref()
                    == Some(module.mod_name.as_str()))
                .then_some(target)
            })
        })
        .collect::<Vec<_>>();

    if canonical_owner_targets.is_empty() {
        return;
    }

    for module in modules.values_mut() {
        let module_name = module.mod_name.clone();
        module.file.items.retain(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return true;
            };
            if !impl_is_compiler_fallback(item_impl) {
                return true;
            }
            let Some(target) = canonical_trait_impl_target(item_impl, &module_name, &module_names)
            else {
                return true;
            };
            !canonical_owner_targets
                .iter()
                .any(|owner| canonical_targets_match(owner, &target))
        });
    }
}

fn impl_is_compiler_fallback(item_impl: &syn::ItemImpl) -> bool {
    generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs)
}

fn items_are_structurally_equivalent(left: &syn::Item, right: &syn::Item) -> bool {
    token_streams_match(
        left.to_token_stream().into_iter(),
        right.to_token_stream().into_iter(),
    )
}

fn token_streams_match(
    mut left: impl Iterator<Item = TokenTree>,
    mut right: impl Iterator<Item = TokenTree>,
) -> bool {
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) if token_trees_match(&left, &right) => {}
            _ => return false,
        }
    }
}

fn token_trees_match(left: &TokenTree, right: &TokenTree) -> bool {
    match (left, right) {
        (TokenTree::Group(left), TokenTree::Group(right)) => {
            left.delimiter() == right.delimiter()
                && token_streams_match(left.stream().into_iter(), right.stream().into_iter())
        }
        (TokenTree::Ident(left), TokenTree::Ident(right)) => {
            let left = left.to_string();
            let right = right.to_string();
            left == right
        }
        (TokenTree::Punct(left), TokenTree::Punct(right)) => {
            left.as_char() == right.as_char() && left.spacing() == right.spacing()
        }
        (TokenTree::Literal(left), TokenTree::Literal(right)) => {
            left.to_string() == right.to_string()
        }
        _ => false,
    }
}

struct CanonicalTraitImplTarget {
    trait_path: syn::Path,
    self_ty: syn::Type,
}

fn canonical_trait_impl_target(
    item_impl: &syn::ItemImpl,
    current_module: &str,
    module_names: &HashSet<String>,
) -> Option<CanonicalTraitImplTarget> {
    let (polarity, trait_path, _) = item_impl.trait_.as_ref()?;
    if polarity.is_some() {
        return None;
    }
    let generic_names = item_impl
        .generics
        .params
        .iter()
        .map(|param| match param {
            syn::GenericParam::Lifetime(param) => param.lifetime.ident.to_string(),
            syn::GenericParam::Type(param) => param.ident.to_string(),
            syn::GenericParam::Const(param) => param.ident.to_string(),
        })
        .collect::<HashSet<_>>();
    let mut qualifier = LocalPathQualifier {
        current_module,
        module_names,
        generic_names: &generic_names,
    };
    let mut trait_path = trait_path.clone();
    qualifier.visit_path_mut(&mut trait_path);
    let mut self_ty = (*item_impl.self_ty).clone();
    qualifier.visit_type_mut(&mut self_ty);
    Some(CanonicalTraitImplTarget {
        trait_path,
        self_ty,
    })
}

fn canonical_targets_match(
    left: &CanonicalTraitImplTarget,
    right: &CanonicalTraitImplTarget,
) -> bool {
    syn_inspect::syn_path_matches(&left.trait_path, &right.trait_path)
        && syn_inspect::syn_type_matches(&left.self_ty, &right.self_ty)
}

pub(super) fn impl_trait_targets_match_across_modules(
    left: &syn::Item,
    left_module: &str,
    right: &syn::Item,
    right_module: &str,
    module_names: &HashSet<String>,
) -> bool {
    let (syn::Item::Impl(left), syn::Item::Impl(right)) = (left, right) else {
        return false;
    };
    let (Some(left), Some(right)) = (
        canonical_trait_impl_target(left, left_module, module_names),
        canonical_trait_impl_target(right, right_module, module_names),
    ) else {
        return false;
    };
    canonical_targets_match(&left, &right)
}

struct LocalPathQualifier<'a> {
    current_module: &'a str,
    module_names: &'a HashSet<String>,
    generic_names: &'a HashSet<String>,
}

impl VisitMut for LocalPathQualifier<'_> {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        visit_mut::visit_path_mut(self, path);
        if path.leading_colon.is_some() || path.segments.is_empty() {
            return;
        }
        let Some(first) = path
            .segments
            .first()
            .map(|segment| segment.ident.to_string())
        else {
            return;
        };
        if first == "crate" {
            qualify_crate_root_main_type(path, self.module_names);
            return;
        }
        if path_root_is_absolute(&first)
            || self.generic_names.contains(&first)
            || (path.segments.len() == 1 && single_segment_type_is_ambient(&first))
        {
            return;
        }

        let module = if self.module_names.contains(&first) {
            None
        } else {
            Some(self.current_module)
        };
        let mut prefix = syn::punctuated::Punctuated::new();
        prefix.push(syn::PathSegment::from(syn::Ident::new(
            "crate",
            proc_macro2::Span::mixed_site(),
        )));
        if let Some(module) = module {
            prefix.push(syn::PathSegment::from(syn::Ident::new(
                module,
                proc_macro2::Span::mixed_site(),
            )));
        }
        prefix.extend(std::mem::take(&mut path.segments));
        path.segments = prefix;
    }
}

/// Normalize generated main-package types to a synthetic module-qualified
/// identity while comparing impl targets.
///
/// Main items are printed at the crate root, while local paths inside the main
/// package are naturally qualified as `crate::main::Type` by this comparison
/// pass. Consumer fallbacks already name those items as `crate::Type`. Insert
/// the synthetic `main` segment only for crate-root paths whose first item is
/// not an actual generated module so both spellings share one ownership key.
fn qualify_crate_root_main_type(path: &mut syn::Path, module_names: &HashSet<String>) {
    if !module_names.contains("main") || path.segments.len() < 2 {
        return;
    }
    let Some(root_item) = path.segments.iter().nth(1) else {
        return;
    };
    if module_names.contains(&root_item.ident.to_string()) {
        return;
    }

    let segments = std::mem::take(&mut path.segments);
    let mut iter = segments.into_iter();
    let Some(crate_segment) = iter.next() else {
        return;
    };
    let mut qualified = syn::punctuated::Punctuated::new();
    qualified.push(crate_segment);
    qualified.push(syn::PathSegment::from(syn::Ident::new(
        "main",
        proc_macro2::Span::mixed_site(),
    )));
    qualified.extend(iter);
    path.segments = qualified;
}

fn path_root_is_absolute(root: &str) -> bool {
    matches!(root, "crate" | "self" | "super" | "std" | "core" | "alloc")
}

fn single_segment_type_is_ambient(name: &str) -> bool {
    matches!(
        name,
        "Self"
            | "bool"
            | "char"
            | "str"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "Box"
            | "Option"
            | "Result"
            | "String"
            | "Vec"
    )
}

fn self_type_owner_module(ty: &syn::Type, module_names: &HashSet<String>) -> Option<String> {
    match ty {
        syn::Type::Reference(reference) => self_type_owner_module(&reference.elem, module_names),
        syn::Type::Paren(paren) => self_type_owner_module(&paren.elem, module_names),
        syn::Type::Group(group) => self_type_owner_module(&group.elem, module_names),
        syn::Type::Path(path) if path.qself.is_none() => {
            let segments = path.path.segments.iter().collect::<Vec<_>>();
            if path_is_transparent_storage_wrapper(&segments)
                && let Some(inner) = first_type_argument(segments.last()?)
            {
                return self_type_owner_module(inner, module_names);
            }
            match segments.as_slice() {
                [crate_segment, module, ..]
                    if crate_segment.ident == "crate"
                        && module_names.contains(&module.ident.to_string()) =>
                {
                    Some(module.ident.to_string())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn path_is_transparent_storage_wrapper(segments: &[&syn::PathSegment]) -> bool {
    let names = segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    matches!(
        names.as_slice(),
        [crate_, builtin, wrapper]
            if crate_ == "crate" && builtin == "builtin" && wrapper == "GorsPtr"
    ) || matches!(
        names.as_slice(),
        [std, sync, wrapper]
            if std == "std" && sync == "sync" && matches!(wrapper.as_str(), "Arc" | "Mutex" | "LazyLock")
    ) || matches!(names.as_slice(), [wrapper] if wrapper == "Box")
}

fn first_type_argument(segment: &syn::PathSegment) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::TypeEnv;

    fn trait_impl_count(items: &[syn::Item], expected: &syn::Item) -> usize {
        items
            .iter()
            .filter(|item| syn_inspect::impl_trait_targets_match(item, expected))
            .count()
    }

    #[test]
    fn equivalent_impl_dedup_keeps_divergent_same_target_impls() {
        let duplicate: syn::Item = syn::parse_quote! {
            impl Reader for Source {
                fn Read(&mut self) -> isize { 1 }
            }
        };
        let divergent: syn::Item = syn::parse_quote! {
            impl Reader for Source {
                fn Read(&mut self) -> isize { 2 }
            }
        };
        let mut items = vec![duplicate.clone(), duplicate.clone(), divergent];

        dedupe_equivalent_trait_impls(&mut items);

        assert_eq!(trait_impl_count(&items, &duplicate), 2);
    }

    #[test]
    fn cross_file_inherited_interface_impls_dedupe_after_package_merge() {
        let interface_file = crate::parser::parse_file(
            "interfaces.go",
            r#"
package streams

type Reader interface { Read([]byte) (int, error) }
type Closer interface { Close() error }
type ReadCloser interface {
	Reader
	Closer
}

type Source struct{}

func (s *Source) Read(p []byte) (int, error) { return len(p), nil }
func First(s *Source) ReadCloser { return s }
"#,
        )
        .unwrap();
        let closer_file = crate::parser::parse_file(
            "closer.go",
            r#"
package streams

func (s *Source) Close() error { return nil }
func Second(s *Source) ReadCloser { return s }
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_files(&[&interface_file, &closer_file]);
        let mut items = Vec::new();
        for file in [interface_file, closer_file] {
            items.extend(
                crate::compiler::compile_with_type_env(file, env.clone())
                    .unwrap()
                    .items,
            );
        }
        let target: syn::Item = syn::parse_quote! {
            impl ReadCloser for crate::builtin::GorsPtr<Source> {}
        };

        assert!(trait_impl_count(&items, &target) > 1);
        dedupe_equivalent_trait_impls(&mut items);
        assert_eq!(trait_impl_count(&items, &target), 1);
    }

    #[test]
    fn final_module_path_normalization_exposes_equivalent_fallback_impls() {
        let relative: syn::Item = syn::parse_quote! {
            #[doc = "gors:external-local-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl Reader for bytes::Buffer {
                fn Read(&mut self) {}
            }
        };
        let qualified: syn::Item = syn::parse_quote! {
            #[doc = "gors:external-local-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl Reader for crate::bytes::Buffer {
                fn Read(&mut self) {}
            }
        };
        let mut modules = BTreeMap::from([
            ("bytes".to_string(), module("bytes", Vec::new())),
            (
                "compress".to_string(),
                module("compress", vec![relative, qualified]),
            ),
        ]);

        super::super::prefix_final_module_paths(&mut modules);
        dedupe_program_trait_impls(&mut modules);

        let impls = modules["compress"]
            .file
            .items
            .iter()
            .filter(|item| matches!(item, syn::Item::Impl(item_impl) if item_impl.trait_.is_some()))
            .count();
        assert_eq!(impls, 1);
    }

    fn module(name: &str, items: Vec<syn::Item>) -> CompiledModule {
        CompiledModule {
            mod_name: name.to_string(),
            import_path: name.to_string(),
            file: syn::File {
                shebang: None,
                attrs: vec![],
                items,
            },
            filename: format!("{name}.rs"),
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        }
    }

    #[test]
    fn dce_preserved_canonical_impl_is_not_a_removable_fallback() {
        let mut owner_impl: syn::ItemImpl = syn::parse_quote! {
            impl Reader for crate::builtin::GorsPtr<SectionReader> {
                fn Read(&mut self) {}
            }
        };
        generated_attrs::preserve_for_dce(&mut owner_impl.attrs);

        assert!(generated_attrs::attrs_preserve_for_dce(&owner_impl.attrs));
        assert!(!impl_is_compiler_fallback(&owner_impl));

        let mut modules = BTreeMap::from([(
            "io".to_string(),
            module("io", vec![syn::Item::Impl(owner_impl)]),
        )]);
        dedupe_program_trait_impls(&mut modules);

        assert_eq!(modules["io"].file.items.len(), 1);
    }

    #[test]
    fn program_dedup_removes_dedicated_fallback_only_when_owner_exists() {
        let owner_impl: syn::Item = syn::parse_quote! {
            impl Reader for crate::builtin::GorsPtr<SectionReader> {
                fn Read(&mut self) { self.interface_key(); }
            }
        };
        let marked_fallback: syn::Item = syn::parse_quote! {
            #[allow(dead_code)]
            #[doc = "gors:preserve-imported-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl crate::io::Reader for crate::builtin::GorsPtr<crate::io::SectionReader> {
                fn Read(&mut self) { crate::builtin::GorsInterfaceKey::non_comparable::<Self>(); }
            }
        };
        let unmarked_conflict: syn::Item = syn::parse_quote! {
            impl crate::io::Reader for crate::builtin::GorsPtr<crate::io::SectionReader> {
                fn Read(&mut self) { panic!(); }
            }
        };
        let missing_owner_fallback: syn::Item = syn::parse_quote! {
            #[allow(dead_code)]
            #[doc = "gors:preserve-imported-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl crate::io::Reader for crate::builtin::GorsPtr<crate::io::OtherReader> {}
        };
        let mut modules = BTreeMap::from([
            ("io".to_string(), module("io", vec![owner_impl])),
            (
                "zip".to_string(),
                module(
                    "zip",
                    vec![marked_fallback, unmarked_conflict, missing_owner_fallback],
                ),
            ),
        ]);

        dedupe_program_trait_impls(&mut modules);

        let zip_items = &modules["zip"].file.items;
        assert_eq!(zip_items.len(), 2);
        assert!(zip_items.iter().any(|item| {
            matches!(item, syn::Item::Impl(item_impl)
                if !generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs))
        }));
        assert!(zip_items.iter().any(|item| {
            matches!(item, syn::Item::Impl(item_impl)
                if generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs))
        }));
    }

    #[test]
    fn program_dedup_removes_external_local_fallback_when_owner_impl_exists() {
        let owner_impl: syn::Item = syn::parse_quote! {
            impl Locker for crate::builtin::GorsPtr<Mutex> {
                fn Lock(&mut self) {}
            }
        };
        let marked_fallback: syn::Item = syn::parse_quote! {
            #[allow(dead_code)]
            #[doc = "gors:external-local-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl crate::sync::Locker for crate::builtin::GorsPtr<crate::sync::Mutex> {
                fn Lock(&mut self) {}
            }
        };
        let mut modules = BTreeMap::from([
            ("sync".to_string(), module("sync", vec![owner_impl])),
            (
                "consumer".to_string(),
                module("consumer", vec![marked_fallback]),
            ),
        ]);

        dedupe_program_trait_impls(&mut modules);

        assert!(modules["consumer"].file.items.is_empty());
        assert_eq!(modules["sync"].file.items.len(), 1);
    }

    #[test]
    fn program_dedup_matches_crate_root_main_types_to_main_owner_impls() {
        let owner_impl: syn::Item = syn::parse_quote! {
            impl io::Writer for crate::builtin::GorsPtr<passthroughWriteCloser> {
                fn Write(&mut self) {}
            }
        };
        let fallback: syn::Item = syn::parse_quote! {
            #[doc = "gors:external-local-interface-impl"]
            #[doc = "gors:removable-interface-fallback"]
            impl Writer for crate::builtin::GorsPtr<crate::passthroughWriteCloser> {
                fn Write(&mut self) {}
            }
        };
        let mut modules = BTreeMap::from([
            ("__main__".to_string(), module("main", vec![owner_impl])),
            ("io".to_string(), module("io", vec![fallback])),
        ]);

        dedupe_program_trait_impls(&mut modules);

        assert_eq!(modules["__main__"].file.items.len(), 1);
        assert!(modules["io"].file.items.is_empty());
    }
}
