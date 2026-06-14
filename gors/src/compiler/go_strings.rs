use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use proc_macro2::Span;

use crate::{ast, token};

use super::{TYPE_ENV, ir, selector_path_from_ref, special_type_conversion_kind, synthetic_names};

thread_local! {
    static STRING_CONST_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub(super) fn set_string_const_names(names: HashSet<String>) {
    STRING_CONST_NAMES.with(|current| {
        *current.borrow_mut() = names;
    });
}

pub(super) fn mark_string_const_fn(ident: &syn::Ident) {
    STRING_CONST_NAMES.with(|names| {
        names.borrow_mut().insert(ident.to_string());
    });
}

fn is_string_const_fn(name: &str) -> bool {
    STRING_CONST_NAMES.with(|names| names.borrow().contains(name))
}

pub(super) fn is_active_string_const_fn(name: &str) -> bool {
    is_string_const_fn(name) && TYPE_ENV.with(|env| env.borrow().is_const(name))
}

fn selector_string_const_key(selector_expr: &ast::SelectorExpr) -> Option<String> {
    let ast::Expr::Ident(package) = &*selector_expr.x else {
        return None;
    };
    Some(format!("{}.{}", package.name, selector_expr.sel.name))
}

pub(super) fn is_active_selector_string_const_fn(selector_expr: &ast::SelectorExpr) -> bool {
    selector_string_const_key(selector_expr)
        .as_deref()
        .is_some_and(is_active_string_const_fn)
}

pub(super) fn interpret_go_string_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some('r') => result.push('\r'),
            Some('\\') => result.push('\\'),
            Some('"') => result.push('"'),
            Some('\'') => result.push('\''),
            Some('a') => result.push('\x07'),
            Some('b') => result.push('\x08'),
            Some('f') => result.push('\x0C'),
            Some('v') => result.push('\x0B'),
            Some('0') => result.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(val) = u8::from_str_radix(&hex, 16) {
                    result.push(val as char);
                } else {
                    result.push('\\');
                    result.push('x');
                    result.push_str(&hex);
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push('u');
                    result.push_str(&hex);
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push('U');
                    result.push_str(&hex);
                }
            }
            Some(other) => {
                if other.is_ascii_digit() {
                    let mut oct = String::new();
                    oct.push(other);
                    for _ in 0..2 {
                        if let Some(next) = chars.as_str().chars().next() {
                            if next.is_ascii_digit() {
                                oct.push(chars.next().unwrap_or('0'));
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(val) = u8::from_str_radix(&oct, 8) {
                        result.push(val as char);
                    } else {
                        result.push('\\');
                        result.push_str(&oct);
                    }
                } else {
                    result.push('\\');
                    result.push(other);
                }
            }
            None => result.push('\\'),
        }
    }
    result
}

fn interpreted_go_string_bytes(s: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            result.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }

        match chars.next() {
            Some('n') => result.push(b'\n'),
            Some('t') => result.push(b'\t'),
            Some('r') => result.push(b'\r'),
            Some('\\') => result.push(b'\\'),
            Some('"') => result.push(b'"'),
            Some('\'') => result.push(b'\''),
            Some('a') => result.push(0x07),
            Some('b') => result.push(0x08),
            Some('f') => result.push(0x0c),
            Some('v') => result.push(0x0b),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(val) = u8::from_str_radix(&hex, 16) {
                    result.push(val);
                } else {
                    result.extend_from_slice(b"\\x");
                    result.extend_from_slice(hex.as_bytes());
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    let mut buf = [0u8; 4];
                    result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                } else {
                    result.extend_from_slice(b"\\u");
                    result.extend_from_slice(hex.as_bytes());
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    let mut buf = [0u8; 4];
                    result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                } else {
                    result.extend_from_slice(b"\\U");
                    result.extend_from_slice(hex.as_bytes());
                }
            }
            Some(other) if other.is_ascii_digit() => {
                let mut oct = String::new();
                oct.push(other);
                for _ in 0..2 {
                    if let Some(next) = chars.as_str().chars().next() {
                        if next.is_ascii_digit() {
                            oct.push(chars.next().unwrap_or('0'));
                        } else {
                            break;
                        }
                    }
                }
                if let Ok(val) = u8::from_str_radix(&oct, 8) {
                    result.push(val);
                } else {
                    result.push(b'\\');
                    result.extend_from_slice(oct.as_bytes());
                }
            }
            Some(other) => {
                result.push(b'\\');
                let mut buf = [0u8; 4];
                result.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
            None => result.push(b'\\'),
        }
    }
    result
}

fn go_string_literal_bytes(lit: &ast::BasicLit) -> Option<Vec<u8>> {
    if lit.kind != token::Token::STRING {
        return None;
    }
    let raw = lit.value;
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    if raw.starts_with('`') {
        Some(
            inner
                .as_bytes()
                .iter()
                .copied()
                .filter(|byte| *byte != b'\r')
                .collect(),
        )
    } else {
        Some(interpreted_go_string_bytes(inner))
    }
}

pub(super) fn byte_vec_expr(bytes: &[u8]) -> syn::Expr {
    let elems: Vec<syn::Expr> = bytes
        .iter()
        .map(|byte| {
            let lit = syn::LitInt::new(&format!("{byte}u8"), Span::mixed_site());
            syn::parse_quote! { #lit }
        })
        .collect();
    syn::parse_quote! { Vec::<u8>::from([#(#elems),*]) }
}

pub(super) fn string_const_bytes_fn_ident(name: &str) -> syn::Ident {
    synthetic_names::string_const_bytes_fn_ident(name)
}

fn selector_string_const_bytes_path(selector_expr: &ast::SelectorExpr) -> Option<syn::Path> {
    let mut path = selector_path_from_ref(selector_expr);
    let last = path.segments.last_mut()?;
    last.ident = string_const_bytes_fn_ident(selector_expr.sel.name);
    Some(path)
}

pub(super) fn const_eval_string_bytes(
    expr: &ast::Expr,
    values: &BTreeMap<String, Vec<u8>>,
) -> Option<Vec<u8>> {
    match expr {
        ast::Expr::BasicLit(lit) => go_string_literal_bytes(lit),
        ast::Expr::Ident(ident) => values.get(ident.name).cloned(),
        ast::Expr::ParenExpr(paren) => const_eval_string_bytes(&paren.x, values),
        ast::Expr::BinaryExpr(binary) if binary.op == token::Token::ADD => {
            let mut lhs = const_eval_string_bytes(&binary.x, values)?;
            let rhs = const_eval_string_bytes(&binary.y, values)?;
            lhs.extend(rhs);
            Some(lhs)
        }
        _ => None,
    }
}

pub(super) fn string_bytes_vec_expr_for_expr(expr: &ast::Expr) -> Option<syn::Expr> {
    match expr {
        ast::Expr::BasicLit(lit) => go_string_literal_bytes(lit).map(|bytes| byte_vec_expr(&bytes)),
        ast::Expr::Ident(ident) if is_active_string_const_fn(ident.name) => {
            let ident = string_const_bytes_fn_ident(ident.name);
            Some(syn::parse_quote! { #ident() })
        }
        ast::Expr::SelectorExpr(selector) if is_active_selector_string_const_fn(selector) => {
            let path = selector_string_const_bytes_path(selector)?;
            Some(syn::parse_quote! { #path() })
        }
        ast::Expr::ParenExpr(paren) => string_bytes_vec_expr_for_expr(&paren.x),
        ast::Expr::BinaryExpr(binary) if binary.op == token::Token::ADD => {
            let lhs = string_bytes_vec_expr_for_expr(&binary.x)?;
            let rhs = string_bytes_vec_expr_for_expr(&binary.y)?;
            let string_bytes = synthetic_names::string_bytes_temp_ident();
            Some(syn::parse_quote! {{
                let mut #string_bytes = #lhs;
                #string_bytes.extend_from_slice(&#rhs);
                #string_bytes
            }})
        }
        _ => None,
    }
}

pub(super) fn byte_slice_conversion_bytes_vec_expr(expr: &ast::Expr) -> Option<syn::Expr> {
    match expr {
        ast::Expr::ParenExpr(paren) => byte_slice_conversion_bytes_vec_expr(&paren.x),
        ast::Expr::CallExpr(call) => {
            if special_type_conversion_kind(call) != Some(ir::SpecialTypeConversionKind::ByteSlice)
            {
                return None;
            }
            let args = call.args.as_deref()?;
            let [arg] = args else {
                return None;
            };
            string_bytes_vec_expr_for_expr(arg)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_lit(value: &'static str) -> ast::BasicLit<'static> {
        ast::BasicLit {
            value_pos: token::Position::default(),
            value_end: token::Position::default(),
            kind: token::Token::STRING,
            value,
        }
    }

    #[test]
    fn string_literal_bytes_preserve_non_utf8_escapes() {
        assert_eq!(
            go_string_literal_bytes(&string_lit(r#""\xff\x00A""#)),
            Some(vec![0xff, 0x00, b'A'])
        );
    }

    #[test]
    fn raw_string_literal_bytes_strip_carriage_returns() {
        assert_eq!(
            go_string_literal_bytes(&string_lit("`a\r\nb`")),
            Some(vec![b'a', b'\n', b'b'])
        );
    }

    #[test]
    fn const_string_bytes_concatenate_literals_and_prior_values() {
        let values = BTreeMap::from([(String::from("Prefix"), vec![0xff])]);
        let expr = ast::Expr::BinaryExpr(ast::BinaryExpr {
            x: Box::new(ast::Expr::Ident(ast::Ident {
                name_pos: token::Position::default(),
                name: "Prefix",
                obj: None,
            })),
            op_pos: token::Position::default(),
            op: token::Token::ADD,
            y: Box::new(ast::Expr::BasicLit(string_lit(r#""\x00""#))),
        });

        assert_eq!(
            const_eval_string_bytes(&expr, &values),
            Some(vec![0xff, 0x00])
        );
    }
}
