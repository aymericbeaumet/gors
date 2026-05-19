use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct Pool {
                pub New: Option<Box<dyn Fn() -> Box<dyn std::any::Any>>>,
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            impl Pool {
                pub fn Get(&self) -> Box<dyn std::any::Any> {
                    if let Some(ref new_fn) = self.New {
                        new_fn()
                    } else {
                        Box::new(())
                    }
                }

                pub fn Put(&self, _x: Box<dyn std::any::Any>) {
                    // No-op: simplified Pool doesn't recycle
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct Mutex<T> {
                inner: std::sync::Mutex<T>,
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub struct Once {
                inner: std::sync::Once,
            }
        },
    ]
}
