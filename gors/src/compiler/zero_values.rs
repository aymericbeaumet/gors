use super::syn_inspect::is_box_type_with_any_bound;
use super::{ast, typeinfer};

pub(super) fn expr_for_type(expr: &ast::Expr) -> syn::Expr {
    match expr {
        ast::Expr::ParenExpr(paren) => expr_for_type(&paren.x),
        ast::Expr::Ident(id) if matches!(id.name, "any" | "error") => {
            expr_for_type_name(Some(id.name))
        }
        _ if let Some(interface_name) = super::interface_name_from_type_expr(expr) => {
            super::boxed_noop_interface_expr(&interface_name)
        }
        ast::Expr::InterfaceType(_) => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        ast::Expr::FuncType(func_type) => expr_for_func_type(func_type),
        ast::Expr::ArrayType(array_type) if array_type.len.is_some() => {
            expr_for_array_type(array_type)
        }
        _ => syn::parse_quote! { Default::default() },
    }
}

pub(super) fn expr_for_optional_type(type_expr: Option<&ast::Expr>) -> syn::Expr {
    match type_expr {
        Some(expr) if super::interface_name_from_type_expr(expr).is_some() => expr_for_type(expr),
        Some(ast::Expr::Ident(ident)) => expr_for_type_name(Some(ident.name)),
        Some(ast::Expr::InterfaceType(_)) => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        Some(ast::Expr::FuncType(func_type)) => expr_for_func_type(func_type),
        Some(ast::Expr::ArrayType(array_type)) => expr_for_array_type(array_type),
        Some(ast::Expr::MapType(_) | ast::Expr::StarExpr(_)) => {
            syn::parse_quote! { Default::default() }
        }
        Some(_) => syn::parse_quote! { Default::default() },
        None => syn::parse_quote! { 0 },
    }
}

pub(super) fn expr_for_type_name(type_name: Option<&str>) -> syn::Expr {
    match type_name {
        Some("bool") => syn::parse_quote! { false },
        Some("string") => syn::parse_quote! { String::new() },
        Some("float32") | Some("float64") => syn::parse_quote! { 0.0 },
        Some("int") | Some("int8") | Some("int16") | Some("int32") | Some("int64")
        | Some("uint") | Some("uint8") | Some("uint16") | Some("uint32") | Some("uint64")
        | Some("uintptr") | Some("byte") | Some("rune") => syn::parse_quote! { 0 },
        Some("error") => syn::parse_quote! {
            Box::new(crate::builtin::__GorsNooperror::default()) as Box<dyn crate::builtin::error>
        },
        Some("any") => syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> },
        _ => syn::parse_quote! { Default::default() },
    }
}

pub(super) fn expr_for_go_type(go_type: &typeinfer::GoType) -> Option<syn::Expr> {
    match super::resolved_go_type(go_type) {
        typeinfer::GoType::Error => Some(expr_for_type_name(Some("error"))),
        typeinfer::GoType::Any => Some(expr_for_type_name(Some("any"))),
        typeinfer::GoType::Bool => Some(expr_for_type_name(Some("bool"))),
        typeinfer::GoType::String => Some(expr_for_type_name(Some("string"))),
        typeinfer::GoType::Unit => Some(syn::parse_quote! { () }),
        typeinfer::GoType::Interface(_) => None,
        _ => Some(syn::parse_quote! { Default::default() }),
    }
}

pub(super) fn expr_for_syn_type(ty: Option<&syn::Type>) -> syn::Expr {
    if let Some(ty) = ty {
        if is_box_type_with_any_bound(ty) {
            return expr_for_type_name(Some("any"));
        }
    }
    if matches!(ty, Some(syn::Type::Array(_))) {
        return syn::parse_quote! { std::array::from_fn(|_| Default::default()) };
    }
    if let Some(syn::Type::Path(type_path)) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            let name = seg.ident.to_string();
            match name.as_str() {
                "bool" => return expr_for_type_name(Some("bool")),
                "string" | "String" => return expr_for_type_name(Some("string")),
                "float32" | "float64" | "f32" | "f64" => {
                    return expr_for_type_name(Some("float64"));
                }
                "isize" | "i8" | "i16" | "i32" | "i64" | "usize" | "u8" | "u16" | "u32" | "u64" => {
                    return expr_for_type_name(Some("int"));
                }
                "Vec" => return syn::parse_quote! { Vec::new() },
                "HashMap" => return syn::parse_quote! { std::collections::HashMap::new() },
                _ => {}
            }
        }
    }
    if ty.is_some() {
        syn::parse_quote! { Default::default() }
    } else {
        syn::parse_quote! { 0 }
    }
}

fn expr_for_func_type(func_type: &ast::FuncType<'_>) -> syn::Expr {
    let box_ty = super::shared_func_box_type_from_ast(func_type);
    syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(None::<#box_ty>)) }
}

pub(super) fn expr_for_array_type(array_type: &ast::ArrayType) -> syn::Expr {
    if array_type.len.is_none() {
        return syn::parse_quote! { Default::default() };
    }
    let elem_default = expr_for_type(&array_type.elt);
    syn::parse_quote! { std::array::from_fn(|_| #elem_default) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    fn first_type_expr<'src>(source: &'src str) -> Result<ast::Expr<'src>, String> {
        let file = crate::parser::parse_file("fixture.go", source)
            .map_err(|err| format!("failed to parse fixture: {err}"))?;
        file.decls
            .into_iter()
            .find_map(|decl| match decl {
                ast::Decl::GenDecl(decl) => decl.specs.into_iter().find_map(|spec| match spec {
                    ast::Spec::TypeSpec(spec) => Some(spec.type_),
                    ast::Spec::ImportSpec(_) | ast::Spec::ValueSpec(_) => None,
                }),
                ast::Decl::FuncDecl(_) => None,
            })
            .ok_or_else(|| "expected type declaration".to_owned())
    }

    #[test]
    fn any_zero_value_is_boxed_unit() -> Result<(), String> {
        let expr = first_type_expr("package p\ntype T any\n")?;
        let zero = expr_for_type(&expr).to_token_stream().to_string();

        assert!(zero.contains("Box :: new (())"), "{zero}");
        assert!(zero.contains("Box < dyn std :: any :: Any >"), "{zero}");
        Ok(())
    }

    #[test]
    fn func_zero_value_is_shared_nil_cell() -> Result<(), String> {
        let expr = first_type_expr("package p\ntype T func(int) string\n")?;
        let zero = expr_for_type(&expr).to_token_stream().to_string();

        assert!(zero.contains("Arc :: new"), "{zero}");
        assert!(zero.contains("Mutex :: new"), "{zero}");
        assert!(zero.contains("None :: <"), "{zero}");
        Ok(())
    }

    #[test]
    fn fixed_array_zero_value_initializes_elements() -> Result<(), String> {
        let expr = first_type_expr("package p\ntype T [2]any\n")?;
        let zero = expr_for_type(&expr).to_token_stream().to_string();

        assert!(zero.contains("std :: array :: from_fn"), "{zero}");
        assert!(zero.contains("Box :: new (())"), "{zero}");
        Ok(())
    }

    #[test]
    fn optional_string_type_zero_value_is_owned_string() -> Result<(), String> {
        let expr = first_type_expr("package p\ntype T string\n")?;
        let zero = expr_for_optional_type(Some(&expr))
            .to_token_stream()
            .to_string();

        assert_eq!(zero, "String :: new ()");
        Ok(())
    }

    #[test]
    fn ast_primitive_type_defaults_stay_contextual() -> Result<(), String> {
        let expr = first_type_expr("package p\ntype T string\n")?;
        let zero = expr_for_type(&expr).to_token_stream().to_string();

        assert_eq!(zero, "Default :: default ()");
        Ok(())
    }

    #[test]
    fn go_type_zero_values_preserve_unknown_interfaces() {
        let string_zero = expr_for_go_type(&typeinfer::GoType::String)
            .map(|expr| expr.to_token_stream().to_string());
        let interface_zero = expr_for_go_type(&typeinfer::GoType::Interface("io.Writer".into()));

        assert_eq!(string_zero.as_deref(), Some("String :: new ()"));
        assert!(interface_zero.is_none());
    }

    #[test]
    fn rust_array_type_zero_value_uses_from_fn() {
        let ty: syn::Type = syn::parse_quote! { [String; 2] };
        let zero = expr_for_syn_type(Some(&ty)).to_token_stream().to_string();

        assert!(zero.contains("std :: array :: from_fn"), "{zero}");
        assert!(!zero.contains("[Default :: default () ;"), "{zero}");
    }
}
