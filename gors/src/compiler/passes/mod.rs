mod coerce_types;
mod simplify_return;

pub fn pass(file: &mut syn::File) {
    simplify_return::pass(file);
    coerce_types::pass(file);
}

pub fn pass_for_imported_package(file: &mut syn::File) {
    simplify_return::pass(file);
    coerce_types::pass(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    coerce_types::pass_after_package_merge(file);
}

pub fn pass_after_structural_helpers(file: &mut syn::File) {
    coerce_types::pass_after_structural_helpers(file);
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    //! This module contains the passes tests (the Rust -> Rust step, after the initial Go -> Rust
    //! step).

    use super::pass;
    use quote::quote;
    use syn::parse_quote as rust;

    fn test(mut input: syn::File, expected: syn::File) {
        pass(&mut input); // mutates in place, becomes the output
        let output = (quote! {#input}).to_string();
        let expected = (quote! {#expected}).to_string();
        assert_eq!(output, expected);
    }

    #[test]
    fn it_should_remove_unnecessary_returns() {
        test(rust! { fn a() { return 0; } }, rust! { fn a() { 0 } });
        test(rust! { fn a() { return 0 } }, rust! { fn a() { 0 } });
        test(
            rust! { fn a() { if true { return 0; } return 2; } },
            rust! { fn a() { if true { return 0; } 2 } },
        );
    }
}
