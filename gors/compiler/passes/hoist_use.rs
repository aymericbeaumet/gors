use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    let mut p = HoistUse::default();
    p.visit_file_mut(file);

    for (i, u) in p.uses.into_values().enumerate() {
        file.items.insert(i, u);
    }
}

#[derive(Default)]
struct HoistUse {
    uses: std::collections::BTreeMap<String, syn::Item>,
}

impl VisitMut for HoistUse {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if path.segments.len() > 1 {
            // Save the "use" to hoist it later
            self.uses.insert(
                (quote::quote! { #path }).to_string(),
                syn::parse_quote! { use #path; },
            );

            // Trim the pass segment to only keep the latest element
            let ident = path.segments.last().unwrap();
            path.leading_colon = None;
            path.segments = syn::parse_quote! { #ident };
        }
    }
}
