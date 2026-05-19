use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Println<T: ::std::fmt::Display>(a: T) {
                ::std::println!("{}", a);
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Print<T: ::std::fmt::Display>(a: T) {
                ::std::print!("{}", a);
            }
        },
    ]
}
