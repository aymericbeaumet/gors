mod avoid_item_shadowing;
mod coerce_types;
mod flatten_block;
mod hoist_use;
mod index_cast;
mod inject_channel;
mod map_type;
mod nil_check;
mod simplify_return;
mod string_lit;
mod type_conversion;

pub fn pass(file: &mut syn::File) {
    map_type::pass(file);
    type_conversion::pass(file);
    inject_channel::pass(file);
    nil_check::pass(file);
    string_lit::pass(file);
    hoist_use::pass(file);
    simplify_return::pass(file);
    flatten_block::pass(file);
    index_cast::pass(file);
    coerce_types::pass(file);
    avoid_item_shadowing::pass(file);
}

pub fn pass_for_imported_package(file: &mut syn::File) {
    map_type::pass(file);
    type_conversion::pass(file);
    simplify_return::pass(file);
    flatten_block::pass(file);
    index_cast::pass(file);
    coerce_types::pass(file);
    avoid_item_shadowing::pass(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    coerce_types::pass_after_package_merge(file);
    avoid_item_shadowing::pass(file);
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

    #[test]
    fn it_should_hoist_use_declarations() {
        test(
            rust! { fn a() { ::std::println!("hello"); } },
            rust! { use ::std::println; fn a() { println!("hello"); } },
        );
        test(
            rust! { pub fn main() { ::std::println!("test"); } },
            rust! { use ::std::println; pub fn main() { println!("test"); } },
        );
    }

    #[test]
    fn it_should_only_hoist_duplicates_once() {
        test(
            rust! { fn a() { ::std::println!("hello"); ::std::println!("world"); } },
            rust! { use ::std::println; fn a() { println!("hello"); println!("world"); } },
        );
    }

    #[test]
    fn it_should_flatten_blocks_composed_of_one_expr() {
        test(rust! { fn a() { { { a } } } }, rust! { fn a() { a } });
        test(
            rust! { fn a() { { loop { { loop {} } } } } },
            rust! { fn a() { loop { loop {} } } },
        );
        test(rust! { fn a() { { { a; } } } }, rust! { fn a() { { a; } } });
        test(
            rust! { fn a() { { { return a; } } } },
            rust! { fn a() { { return a; } } },
        );
    }
}
