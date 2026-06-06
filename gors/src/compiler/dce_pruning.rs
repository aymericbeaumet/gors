use super::{
    generated_attrs,
    item_reachability::reachable_item_for_names,
    reachability_cache::ReachableItems,
    reachability_names::{item_reachability_names, top_level_item_names},
};

pub(super) fn retain_reachable_items(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
    reachable: &ReachableItems,
) {
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    *items = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            if generated_attrs::item_preserves_for_dce(item) {
                return Some(item.clone());
            }
            reachable.keep.contains(&idx).then(|| {
                reachable_item_for_names(
                    item,
                    &reachable.names,
                    &item_names,
                    &top_level_names,
                    roots,
                )
            })?
        })
        .collect();
}

pub(super) fn prune_unused_struct_fields(
    items: &mut Vec<syn::Item>,
    preserved_structs: &std::collections::HashSet<String>,
) {
    use syn::visit::Visit;
    use syn::visit_mut::VisitMut;

    let declared_fields = declared_named_fields(items);
    if declared_fields.is_empty() {
        return;
    }

    struct FieldUseCollector {
        used: std::collections::HashSet<String>,
    }

    impl<'ast> Visit<'ast> for FieldUseCollector {
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            if let syn::Member::Named(name) = &field.member {
                self.used.insert(name.to_string());
            }
            syn::visit::visit_expr_field(self, field);
        }

        fn visit_field_pat(&mut self, field: &'ast syn::FieldPat) {
            if let syn::Member::Named(name) = &field.member {
                self.used.insert(name.to_string());
            }
            syn::visit::visit_field_pat(self, field);
        }
    }

    let mut collector = FieldUseCollector {
        used: std::collections::HashSet::new(),
    };
    for item in items.iter() {
        collector.visit_item(item);
    }

    let mut removed = std::collections::HashSet::new();
    for item in items.iter_mut() {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let syn::Fields::Named(fields) = &mut item_struct.fields else {
            continue;
        };
        if preserved_structs.contains(&item_struct.ident.to_string()) {
            generated_attrs::allow_dead_code(&mut item_struct.attrs);
            continue;
        }
        let public_struct = matches!(item_struct.vis, syn::Visibility::Public(_));
        fields.named = fields
            .named
            .clone()
            .into_iter()
            .filter_map(|field| {
                let public_field = matches!(field.vis, syn::Visibility::Public(_));
                let keep = field.ident.as_ref().is_none_or(|ident| {
                    collector.used.contains(&ident.to_string()) || (public_struct && public_field)
                });
                if !keep && let Some(ident) = &field.ident {
                    removed.insert(ident.to_string());
                }
                keep.then_some(field)
            })
            .collect();
    }

    if removed.is_empty() {
        return;
    }

    struct StructLiteralPruner<'a> {
        removed: &'a std::collections::HashSet<String>,
    }

    impl VisitMut for StructLiteralPruner<'_> {
        fn visit_expr_struct_mut(&mut self, expr: &mut syn::ExprStruct) {
            syn::visit_mut::visit_expr_struct_mut(self, expr);
            expr.fields = expr
                .fields
                .clone()
                .into_iter()
                .filter(|field| match &field.member {
                    syn::Member::Named(name) => !self.removed.contains(&name.to_string()),
                    syn::Member::Unnamed(_) => true,
                })
                .collect();
        }
    }

    let mut pruner = StructLiteralPruner { removed: &removed };
    for item in items {
        pruner.visit_item_mut(item);
    }
}

fn declared_named_fields(items: &[syn::Item]) -> std::collections::HashSet<String> {
    let mut fields = std::collections::HashSet::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            if let Some(ident) = &field.ident {
                fields.insert(ident.to_string());
            }
        }
    }
    fields
}

pub(super) fn prune_unused_use_items(items: &mut Vec<syn::Item>) {
    use syn::visit::Visit;

    struct UsedIdentCollector {
        used: std::collections::HashSet<String>,
    }

    impl<'ast> Visit<'ast> for UsedIdentCollector {
        fn visit_item_use(&mut self, _item: &'ast syn::ItemUse) {}

        fn visit_path(&mut self, path: &'ast syn::Path) {
            for segment in &path.segments {
                self.used.insert(segment.ident.to_string());
            }
            syn::visit::visit_path(self, path);
        }
    }

    let mut collector = UsedIdentCollector {
        used: std::collections::HashSet::new(),
    };
    for item in items.iter() {
        collector.visit_item(item);
    }

    items.retain_mut(|item| {
        let syn::Item::Use(item_use) = item else {
            return true;
        };
        prune_use_tree(&mut item_use.tree, &collector.used)
    });
}

fn prune_use_tree(tree: &mut syn::UseTree, used: &std::collections::HashSet<String>) -> bool {
    match tree {
        syn::UseTree::Name(name) => used.contains(&name.ident.to_string()),
        syn::UseTree::Rename(rename) => used.contains(&rename.rename.to_string()),
        syn::UseTree::Path(path) => prune_use_tree(&mut path.tree, used),
        syn::UseTree::Group(group) => {
            group.items = group
                .items
                .clone()
                .into_iter()
                .filter_map(|mut tree| prune_use_tree(&mut tree, used).then_some(tree))
                .collect();
            !group.items.is_empty()
        }
        syn::UseTree::Glob(_) => true,
    }
}
