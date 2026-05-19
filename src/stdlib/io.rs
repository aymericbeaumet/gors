use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub trait Writer {
                fn Write(&mut self, p: &[u8]) -> (usize, Option<Box<dyn std::error::Error>>);
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub trait Reader {
                fn Read(&mut self, p: &mut [u8]) -> (usize, Option<Box<dyn std::error::Error>>);
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn WriteString(
                w: &mut dyn Writer,
                s: &str,
            ) -> (usize, Option<Box<dyn std::error::Error>>) {
                w.Write(s.as_bytes())
            }
        },
    ]
}
