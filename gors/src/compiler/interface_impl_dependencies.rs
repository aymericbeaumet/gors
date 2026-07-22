use std::collections::BTreeSet;

/// Record the transitive supertrait obligations of generated interface impls.
///
/// Rust requires every supertrait impl to remain present whenever a composite
/// trait impl is present. Reachability operates on individual impl items, so a
/// composite impl's ordinary body references are not enough to retain its
/// otherwise-unreferenced supertrait impls. These compiler-owned markers keep
/// that structural dependency explicit without preserving unrelated impls.
pub(super) fn mark_required_supertrait_impls(items: &mut [syn::Item]) {
    let relationships = super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        let mut relationships = Vec::new();
        for parent_name in env.interface_names() {
            let parent_path = super::interface_trait_path_from_name(&parent_name);
            let Some(parent_reachability_name) =
                super::item_reachability::trait_path_reachability_name(&parent_path)
            else {
                continue;
            };
            for embedded_name in env.get_interface_embedded_interfaces(&parent_name) {
                relationships.push((
                    super::interface_trait_path_from_name(&embedded_name),
                    parent_reachability_name.clone(),
                ));
            }
        }
        relationships
    });
    if relationships.is_empty() {
        return;
    }

    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some((_, trait_path, _)) = &item_impl.trait_ else {
            continue;
        };
        let required_by = relationships
            .iter()
            .filter_map(|(embedded_path, parent)| {
                super::syn_inspect::syn_path_matches(trait_path, embedded_path)
                    .then_some(parent.clone())
            })
            .collect::<BTreeSet<_>>();
        for parent in required_by {
            super::generated_attrs::mark_interface_impl_required_by(&mut item_impl.attrs, &parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::item_reachability::trait_impl_reachability_name;
    use std::collections::HashSet;

    #[test]
    fn required_supertrait_markers_preserve_exact_qualified_parent_identity() {
        let mut env = crate::compiler::typeinfer::TypeEnv::new();
        for module in ["first_io", "second_io"] {
            let parent = format!("{module}.ReadCloser");
            let embedded = format!("{module}.Reader");
            env.set_interface_methods(&parent, Vec::new());
            env.set_interface_embedded(&parent, vec![embedded.clone()]);
            env.set_interface_methods(&embedded, vec!["Read".to_string()]);
        }
        crate::compiler::set_type_env(env);
        let mut items = vec![
            syn::parse_quote! {
                impl first_io::Reader for Source {}
            },
            syn::parse_quote! {
                impl second_io::Reader for Source {}
            },
        ];

        super::mark_required_supertrait_impls(&mut items);
        crate::compiler::set_type_env(crate::compiler::typeinfer::TypeEnv::new());

        let expectations = [
            ("first_io::ReadCloser", "second_io::ReadCloser"),
            ("second_io::ReadCloser", "first_io::ReadCloser"),
        ];
        assert_eq!(
            items.len(),
            expectations.len(),
            "unexpected impl item count"
        );
        for (index, (item, (expected, unexpected))) in items.iter().zip(expectations).enumerate() {
            assert!(
                matches!(item, syn::Item::Impl(_)),
                "expected impl item at index {index}"
            );
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            let required_by = item_impl
                .attrs
                .iter()
                .filter_map(crate::generated_names::interface_impl_required_by_from_attr)
                .collect::<HashSet<_>>();
            assert!(required_by.contains(expected), "{required_by:?}");
            assert!(!required_by.contains(unexpected), "{required_by:?}");
        }
    }

    #[test]
    fn composed_interface_root_keeps_all_required_supertrait_impls()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
package fixture

type Reader interface { Read() }
type Closer interface { Close() }
type ReadCloser interface {
	Reader
	Closer
}

type Source struct{}

func (Source) Read() {}
func (Source) Close() {}
func BoxSource() ReadCloser { return Source{} }
"#,
        )?;
        let compiled = crate::compiler::compile(file)?;
        let roots = HashSet::from([trait_impl_reachability_name("ReadCloser", "Source")]);
        let reachable = crate::compiler::dce_reachability::reachable_stdlib_items(
            &compiled.items,
            &roots,
            &HashSet::new(),
        );
        let mut retained = compiled.items;
        crate::compiler::dce_pruning::retain_reachable_items(&mut retained, &roots, &reachable);

        for trait_name in ["Reader", "Closer", "ReadCloser"] {
            assert!(
                retained.iter().any(|item| {
                    let syn::Item::Impl(item_impl) = item else {
                        return false;
                    };
                    item_impl
                        .trait_
                        .as_ref()
                        .and_then(|(_, path, _)| path.segments.last())
                        .is_some_and(|segment| segment.ident == trait_name)
                        && crate::compiler::syn_inspect::self_type_reachability_names(
                            &item_impl.self_ty,
                        )
                        .iter()
                        .any(|name| name == "Source")
                }),
                "missing {trait_name} for Source",
            );
        }
        Ok(())
    }

    #[test]
    fn archive_interface_hook_only_composite_propagates_exact_dependency_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
package fixture

type Reader interface { Read() }
type Closer interface { Close() }
type ReadCloser interface {
	Reader
	Closer
}

type Source struct{}

func (Source) Read() {}
func (Source) Close() {}
"#,
        )?;
        let compiled = crate::compiler::compile(file)?;
        let roots = HashSet::from(["ReadCloser".to_string(), "Source".to_string()]);
        let reachable = crate::compiler::dce_reachability::reachable_stdlib_items(
            &compiled.items,
            &roots,
            &HashSet::new(),
        );

        assert!(
            reachable
                .names
                .contains(&trait_impl_reachability_name("ReadCloser", "Source")),
            "{:?}",
            reachable.names
        );
        for trait_name in ["Reader", "Closer"] {
            let impl_root = trait_impl_reachability_name(trait_name, "Source");
            assert!(reachable.names.contains(&impl_root), "missing {impl_root}");
        }
        Ok(())
    }
}
