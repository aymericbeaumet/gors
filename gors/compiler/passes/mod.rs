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
        ]);
    }

    #[test]
    fn it_should_remove_unnecessary_returns_but_only_in_last_func_stmt() {
        test([(
            rust! { fn a() { if true { return 0; } return 2; } },
            rust! { fn a() { if true { return 0; } 2 } },
        )]);
    }

    #[test]
    fn it_should_hoist_use_declarations() {
        test([(
            rust! { fn a() { ::std::println!("hello"); } },
            rust! { use ::std::println; fn a() { println!("hello"); } },
        )]);
    }

    #[test]
    fn it_should_only_hoist_duplicates_once() {
        test([(
            rust! { fn a() { ::std::println!("hello"); ::std::println!("world"); } },
            rust! { use ::std::println; fn a() { println!("hello"); println!("world"); } },
        )]);
    }
}
