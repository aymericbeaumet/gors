use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    let mut p = HoistUse::default();
    p.visit_file_mut(file);

    for (i, u) in p.uses.into_iter().enumerate() {
        file.items.insert(i, u);
    }
}

#[derive(Default)]
struct HoistUse {
    uses: Vec<syn::Item>,
}

impl VisitMut for HoistUse {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if path.segments.len() > 1 {
            let ident = path.segments.last().unwrap();

            // Save the "use" to hoist it later
            self.uses.push(syn::parse_quote! { use #path; });

            // Trim the pass segment to only keep the latest element
            path.leading_colon = None;
            path.segments = syn::parse_quote! { #ident };
        }

        visit_mut::visit_path_mut(self, path);
    }
}
