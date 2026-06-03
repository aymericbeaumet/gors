use super::{
    TYPE_ENV, ast, borrow_pointer_arg_expr, borrow_pointer_call_arg_indices,
    borrowed_address_of_ident_arg_expr, call_signature_param_types, compile_call_arg_with_expected,
    compile_call_function_expr, is_nil_expr, lvalue_expr_from_ref, nil_borrowed_pointer_arg_expr,
    record_mapping, resolved_go_type, selector_base_is_import, should_borrow_pointer_arg_by_shape,
    typeinfer,
};
use proc_macro2::Span;
use std::collections::HashMap;
use syn::Token;

struct CallArg {
    index: usize,
    lvalue: syn::Expr,
    storage: syn::Ident,
}

pub(super) fn needs_writeback(call_expr: &ast::CallExpr) -> bool {
    if call_expr.ellipsis.is_some() {
        return false;
    }
    if matches!(call_expr.fun.as_ref(), ast::Expr::SelectorExpr(selector) if !selector_base_is_import(selector))
    {
        return false;
    }
    let Some(args) = call_expr.args.as_ref() else {
        return false;
    };
    let param_types = call_signature_param_types(call_expr);
    !call_args(args, &param_types).is_empty()
}

pub(super) fn compile_call(call_expr: ast::CallExpr) -> syn::Expr {
    record_mapping(&call_expr.lparen, None);
    let param_types = call_signature_param_types(&call_expr);
    let borrow_pointer_indices = borrow_pointer_call_arg_indices(&call_expr.fun);
    let args = call_expr.args.unwrap_or_default();
    let plans = call_args(&args, &param_types);
    let plan_indices: HashMap<usize, &CallArg> =
        plans.iter().map(|plan| (plan.index, plan)).collect();
    let func = compile_call_function_expr(*call_expr.fun);

    let mut call_args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
    for (idx, arg) in args.into_iter().enumerate() {
        if let Some(plan) = plan_indices.get(&idx) {
            let storage = &plan.storage;
            call_args.push(syn::parse_quote! {
                crate::builtin::reflect_slice_any(#storage.clone())
            });
            continue;
        }

        let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
        let borrow_pointer_by_shape =
            should_borrow_pointer_arg_by_shape(&arg, param_types.get(idx));
        let should_borrow_pointer =
            borrow_pointer_indices.contains(&idx) || borrow_pointer_by_shape;
        if should_borrow_pointer && is_nil_expr(&arg) {
            call_args.push(nil_borrowed_pointer_arg_expr());
            continue;
        }
        if should_borrow_pointer && let Some(arg) = borrowed_address_of_ident_arg_expr(&arg) {
            call_args.push(arg);
            continue;
        }
        let mut arg = compile_call_arg_with_expected(arg, param_types.get(idx), &actual);
        if should_borrow_pointer {
            borrow_pointer_arg_expr(&mut arg, Some(&actual));
        }
        call_args.push(arg);
    }

    let setup: Vec<syn::Stmt> = plans
        .iter()
        .map(|plan| {
            let storage = &plan.storage;
            let lvalue = &plan.lvalue;
            syn::parse_quote! {
                let #storage = std::sync::Arc::new(std::sync::Mutex::new(std::mem::take(&mut #lvalue)));
            }
        })
        .collect();
    let writeback: Vec<syn::Stmt> = plans
        .iter()
        .map(|plan| {
            let storage = &plan.storage;
            let lvalue = &plan.lvalue;
            syn::parse_quote! {
                #lvalue = std::sync::Arc::try_unwrap(#storage)
                    .unwrap_or_else(|_| crate::builtin::panic_value("reflect slice still borrowed"))
                    .into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        })
        .collect();

    syn::parse_quote! {{
        #(#setup)*
        let __gors_reflect_slice_result = #func(#call_args);
        #(#writeback)*
        __gors_reflect_slice_result
    }}
}

fn call_args(args: &[ast::Expr], param_types: &[typeinfer::GoType]) -> Vec<CallArg> {
    args.iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            let lvalue = call_arg_lvalue(arg, param_types.get(index))?;
            Some(CallArg {
                index,
                lvalue,
                storage: syn::Ident::new(
                    &format!("__gors_reflect_slice_{index}"),
                    Span::mixed_site(),
                ),
            })
        })
        .collect()
}

fn call_arg_lvalue(arg: &ast::Expr, expected: Option<&typeinfer::GoType>) -> Option<syn::Expr> {
    if !matches!(expected.map(resolved_go_type), Some(typeinfer::GoType::Any)) {
        return None;
    }
    let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(arg, &env.borrow()));
    if !matches!(resolved_go_type(&actual), typeinfer::GoType::Slice(_)) {
        return None;
    }
    lvalue_expr_from_ref(arg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    fn main_call<'a>(file: &'a ast::File<'a>, index: usize) -> &'a ast::CallExpr<'a> {
        let ast::Decl::FuncDecl(func) = file
            .decls
            .iter()
            .find(|decl| matches!(decl, ast::Decl::FuncDecl(func) if func.name.name == "main"))
            .expect("main func")
        else {
            panic!("expected main func");
        };
        let ast::Stmt::ExprStmt(expr) = func
            .body
            .as_ref()
            .expect("main body")
            .list
            .get(index)
            .expect("main stmt")
        else {
            panic!("expected expr stmt");
        };
        let ast::Expr::CallExpr(call) = &expr.x else {
            panic!("expected call expr");
        };
        call
    }

    #[test]
    fn needs_writeback_for_addressable_slice_passed_to_any_parameter() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func takesAny(v any) {}
                func takesInt(v int) {}

                func main() {
                    takesAny(values)
                    takesInt(1)
                }
            "#,
        )
        .unwrap();
        let mut env = typeinfer::TypeEnv::new();
        env.scan_file(&file);
        env.set_var(
            "values",
            typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Int)),
        );
        super::super::set_type_env(env);

        assert!(needs_writeback(main_call(&file, 0)));
        assert!(!needs_writeback(main_call(&file, 1)));

        super::super::set_type_env(typeinfer::TypeEnv::new());
    }
}
