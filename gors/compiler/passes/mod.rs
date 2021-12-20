mod flatten_block;
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
    flatten_block::pass(file);
    // TODO: remove useless mut
}

#[cfg(test)]
mod tests {
    use syn::parse_quote as rust;

    fn test(mut input: syn::File, expected: syn::File) {
        super::pass(&mut input); // mutates in place, becomes the output
        let output = (quote::quote! {#input}).to_string();
        let expected = (quote::quote! {#expected}).to_string();
        if output != expected {
            panic!("\n    output: {}\n  expected: {}\n", output, expected);
        }
    }

    #[test]
    fn it_should_remove_unnecessary_returns() {
        test(rust! { fn a() { return 0; } }, rust! { fn a() { 0 } });
        test(rust! { fn b() { return 0 } }, rust! { fn b() { 0 } });
    }

    #[test]
    fn it_should_remove_unnecessary_returns_but_only_in_last_func_stmt() {
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
