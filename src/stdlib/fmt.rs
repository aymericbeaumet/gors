use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(dead_code)]
            pub trait GoDisplay {
                fn go_fmt(&self) -> String;
            }
        },
        rust! {
            impl GoDisplay for i8 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for i16 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for i32 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for i64 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for isize { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for u8 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for u16 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for u32 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for u64 { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for usize { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for bool { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            impl GoDisplay for String { fn go_fmt(&self) -> String { self.clone() } }
        },
        rust! {
            impl GoDisplay for &str { fn go_fmt(&self) -> String { self.to_string() } }
        },
        rust! {
            #[allow(dead_code)]
            fn go_fmt_float64(v: f64) -> String {
                if v == v.trunc() && v.abs() < 1e15 && !v.is_infinite() {
                    format!("{}", v as i64)
                } else {
                    let s = format!("{}", v);
                    s
                }
            }
        },
        rust! {
            #[allow(dead_code)]
            fn go_fmt_float32(v: f32) -> String {
                if v == v.trunc() && v.abs() < 1e7 && !v.is_infinite() {
                    format!("{}", v as i32)
                } else {
                    let s = format!("{}", v);
                    s
                }
            }
        },
        rust! {
            impl GoDisplay for f64 { fn go_fmt(&self) -> String { go_fmt_float64(*self) } }
        },
        rust! {
            impl GoDisplay for f32 { fn go_fmt(&self) -> String { go_fmt_float32(*self) } }
        },
        rust! {
            impl<T: GoDisplay> GoDisplay for Vec<T> {
                fn go_fmt(&self) -> String {
                    let inner: Vec<String> = self.iter().map(|x| x.go_fmt()).collect();
                    format!("[{}]", inner.join(" "))
                }
            }
        },
        rust! {
            impl<T: GoDisplay> GoDisplay for Box<T> {
                fn go_fmt(&self) -> String {
                    let inner: &T = &**self;
                    inner.go_fmt()
                }
            }
        },
        rust! {
            impl<T: GoDisplay> GoDisplay for &T {
                fn go_fmt(&self) -> String { (**self).go_fmt() }
            }
        },
        rust! {
            impl GoDisplay for super::builtin::Complex64 {
                fn go_fmt(&self) -> String {
                    format!("({}{:+}i)", go_fmt_float32(self.re), go_fmt_float32(self.im))
                }
            }
        },
        rust! {
            impl GoDisplay for super::builtin::Complex128 {
                fn go_fmt(&self) -> String {
                    format!("({}{:+}i)", go_fmt_float64(self.re), go_fmt_float64(self.im))
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Println<T: GoDisplay>(a: T) {
                ::std::println!("{}", a.go_fmt());
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Print<T: GoDisplay>(a: T) {
                ::std::print!("{}", a.go_fmt());
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Sprintf<T: GoDisplay>(format: &str, a: T) -> String {
                format.replace("%v", &a.go_fmt())
                    .replace("%d", &a.go_fmt())
                    .replace("%s", &a.go_fmt())
            }
        },
    ]
}
