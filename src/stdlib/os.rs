use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct File {
                inner: std::fs::File,
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub static Stdout: StdoutWrapper = StdoutWrapper;
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub static Stderr: StderrWrapper = StderrWrapper;
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct StdoutWrapper;
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct StderrWrapper;
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            impl StdoutWrapper {
                pub fn Write(&self, p: &[u8]) -> (usize, Option<Box<dyn std::error::Error>>) {
                    use std::io::Write;
                    match std::io::stdout().write_all(p) {
                        Ok(()) => (p.len(), None),
                        Err(e) => (0, Some(Box::new(e))),
                    }
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            impl StderrWrapper {
                pub fn Write(&self, p: &[u8]) -> (usize, Option<Box<dyn std::error::Error>>) {
                    use std::io::Write;
                    match std::io::stderr().write_all(p) {
                        Ok(()) => (p.len(), None),
                        Err(e) => (0, Some(Box::new(e))),
                    }
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Exit(code: i32) {
                std::process::exit(code);
            }
        },
    ]
}
