mod coerce_types;

pub fn pass(file: &mut syn::File) {
    coerce_types::pass(file);
}

pub fn pass_for_imported_package(file: &mut syn::File) {
    coerce_types::pass(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    coerce_types::pass_after_package_merge(file);
}

pub fn pass_after_structural_helpers(file: &mut syn::File) {
    coerce_types::pass_after_structural_helpers(file);
}
