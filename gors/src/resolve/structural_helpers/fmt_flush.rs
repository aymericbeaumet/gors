use super::{StructuralHelperFacts, has_method};

pub(super) fn inject(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if facts.has_pp && !has_method(items, "pp", "__gors_flush_fmt") {
        items.insert(
            0,
            syn::parse_quote! {
                impl pp {
                    fn __gors_flush_fmt(&mut self) {
                        let bytes = std::mem::take(&mut self.fmt.buf.lock().unwrap().0);
                        self.buf.0.extend(bytes);
                    }
                }
            },
        );
    }
}
