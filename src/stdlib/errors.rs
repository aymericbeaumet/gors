use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn New(text: &str) -> Box<dyn std::error::Error> {
                Box::new(SimpleError(text.to_string()))
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            #[derive(Debug)]
            struct SimpleError(String);
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            impl std::fmt::Display for SimpleError {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", self.0)
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            impl std::error::Error for SimpleError {}
        },
    ]
}
