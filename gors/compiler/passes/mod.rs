mod hoist_use;
mod inline_fmt;
mod map_type;
mod simplify_return;
mod type_conversion;

pub fn pass(file: &mut syn::File) {
    inline_fmt::pass(file);
    map_type::pass(file);
    type_conversion::pass(file);
    hoist_use::pass(file);
    simplify_return::pass(file);
    // TODO: block with one element -> removes {}
}

#[cfg(test)]
mod tests {
    use syn::parse_quote as rust;

    fn test<T: std::iter::IntoIterator<Item = (syn::File, syn::File)>>(tests: T) {
        for (mut input, expected) in tests {
            super::pass(&mut input); // mutates in place
            assert_eq!(
                (quote::quote! {#expected}).to_string(),
                (quote::quote! {#input}).to_string()
            );
        }
    }

    #[test]
    fn it_should_remove_unnecessary_returns() {
        test([
            (rust! { fn a() { return 0; } }, rust! { fn a() { 0 } }),
            (rust! { fn b() { return 0 } }, rust! { fn b() { 0 } }),
            (
                rust! { fn c() { if true { return 0; } return 2; } },
                rust! { fn c() { if true { return 0; } 2 } },
            ),
        ]);
    }
}
