use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    let mut collector = CollectUses::default();
    collector.visit_file_mut(file);

    // Detect name conflicts: if multiple paths resolve to the same short name,
    // don't hoist any of them.
    let mut name_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for path_str in collector.uses.keys() {
        if let Some(short_name) = path_str.rsplit(":: ").next() {
            *name_counts.entry(short_name.to_string()).or_insert(0) += 1;
        }
    }

    let mut hoister = Hoist {
        uses: &collector.uses,
        name_counts: &name_counts,
    };
    hoister.visit_file_mut(file);

    let mut insert_idx = 0;
    for (path_str, use_item) in &collector.uses {
        if let Some(short_name) = path_str.rsplit(":: ").next() {
            if name_counts.get(short_name).copied().unwrap_or(0) <= 1 {
                file.items.insert(insert_idx, use_item.clone());
                insert_idx += 1;
            }
        }
    }
}

#[derive(Default)]
struct CollectUses {
    uses: std::collections::BTreeMap<String, syn::Item>,
}

impl VisitMut for CollectUses {
    // Skip descending into module definitions — their internal paths should not be hoisted
    fn visit_item_mod_mut(&mut self, _: &mut syn::ItemMod) {
        // Do not recurse into mod blocks
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, path);

        let has_generics = path
            .segments
            .iter()
            .any(|s| !matches!(s.arguments, syn::PathArguments::None));

        // Skip type method paths like Box::new — those are type-associated, not modules
        let is_type_method_path = path.segments.len() == 2
            && path.leading_colon.is_none()
            && path.segments.first().is_some_and(|s| {
                let name = s.ident.to_string();
                name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            });

        if path.segments.len() > 1
            && !has_generics
            && !is_type_method_path
            && path.leading_colon.is_some()
        {
            self.uses.insert(
                (quote::quote! { #path }).to_string(),
                syn::parse_quote! { use #path; },
            );
        }
    }
}

struct Hoist<'a> {
    uses: &'a std::collections::BTreeMap<String, syn::Item>,
    name_counts: &'a std::collections::HashMap<String, usize>,
}

impl VisitMut for Hoist<'_> {
    // Skip descending into module definitions
    fn visit_item_mod_mut(&mut self, _: &mut syn::ItemMod) {
        // Do not recurse into mod blocks
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, path);

        if path.segments.len() > 1 {
            let path_str = (quote::quote! { #path }).to_string();
            if self.uses.contains_key(&path_str) {
                if let Some(short_name) = path_str.rsplit(":: ").next() {
                    if self.name_counts.get(short_name).copied().unwrap_or(0) <= 1 {
                        if let Some(ident) = path.segments.last() {
                            let ident = ident.clone();
                            path.leading_colon = None;
                            path.segments = syn::parse_quote! { #ident };
                        }
                    }
                }
            }
        }
    }
}
