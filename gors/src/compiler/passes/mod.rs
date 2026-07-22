mod coerce_types;

pub fn pass(file: &mut syn::File) {
    coerce_types::pass(file);
}

pub fn pass_for_imported_package(file: &mut syn::File) {
    coerce_types::pass(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    super::trait_impl_dedup::dedupe_equivalent_trait_impls(&mut file.items);
    coerce_types::pass_after_package_merge(file);
}

pub fn pass_after_structural_helpers(file: &mut syn::File) {
    coerce_types::pass_after_structural_helpers(file);
}
