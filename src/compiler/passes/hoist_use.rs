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
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, path);

        if path.segments.len() > 1 {
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
