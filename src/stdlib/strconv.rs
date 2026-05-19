use syn::parse_quote as rust;

pub fn module_items() -> Vec<syn::Item> {
    vec![
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Itoa(i: isize) -> String {
                i.to_string()
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn Atoi(s: &str) -> (isize, Option<Box<dyn std::error::Error>>) {
                match s.parse::<isize>() {
                    Ok(n) => (n, None),
                    Err(e) => (0, Some(Box::new(e))),
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn FormatInt(i: i64, base: isize) -> String {
                match base {
                    2 => format!("{:b}", i),
                    8 => format!("{:o}", i),
                    16 => format!("{:x}", i),
                    _ => i.to_string(),
                }
            }
        },
        rust! {
            #[allow(non_snake_case, dead_code)]
            pub fn FormatFloat(f: f64, fmt: u32, prec: isize, _bit_size: isize) -> String {
                if prec >= 0 {
                    format!("{:.prec$}", f, prec = prec as usize)
                } else {
                    format!("{}", f)
                }
            }
        },
    ]
}
