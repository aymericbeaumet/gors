use super::ast;

pub(super) fn expr_for_type(expr: &ast::Expr) -> syn::Expr {
    match expr {
        ast::Expr::Ident(id) if id.name == "any" => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        ast::Expr::Ident(id) if id.name == "error" => {
            syn::parse_quote! {
                Box::new(crate::builtin::__GorsNooperror::default()) as Box<dyn crate::builtin::error>
            }
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
}
