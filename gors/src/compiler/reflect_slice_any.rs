use super::import_context::selector_base_is_import;
use super::{
    TYPE_ENV, ast, call_signature_param_types, compile_call_arg_with_expected,
    compile_call_function_expr, lvalue_expr_from_ref, record_mapping, resolved_go_type, typeinfer,
};
use proc_macro2::Span;
use std::collections::HashMap;
use syn::Token;

struct ReflectSliceWritebackPlan {
    param_types: Vec<typeinfer::GoType>,
    args: Vec<WritebackArg>,
}

struct WritebackArg {
    index: usize,
    lvalue: syn::Expr,
    storage: syn::Ident,
}

impl ReflectSliceWritebackPlan {
    fn from_call(call_expr: &ast::CallExpr) -> Option<Self> {
        if call_expr.ellipsis.is_some() {
            return None;
        }
        if matches!(call_expr.fun.as_ref(), ast::Expr::SelectorExpr(selector) if !selector_base_is_import(selector))
        {
            return None;
        }
        let args = call_expr.args.as_ref()?;
        let param_types = call_signature_param_types(call_expr);
        let args = writeback_args(args, &param_types);
        (!args.is_empty()).then_some(Self { param_types, args })
    }
}

pub(super) fn needs_writeback(call_expr: &ast::CallExpr) -> bool {
    ReflectSliceWritebackPlan::from_call(call_expr).is_some()
}

pub(super) fn compile_call(call_expr: ast::CallExpr) -> syn::Expr {
    record_mapping(&call_expr.lparen, None);
    let plan = ReflectSliceWritebackPlan::from_call(&call_expr).unwrap_or_else(|| {
        ReflectSliceWritebackPlan {
            param_types: call_signature_param_types(&call_expr),
            args: Vec::new(),
        }
    });
    let args = call_expr.args.unwrap_or_default();
    let plan_indices: HashMap<usize, &WritebackArg> =
        plan.args.iter().map(|plan| (plan.index, plan)).collect();
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
        let arg = compile_call_arg_with_expected(arg, plan.param_types.get(idx), &actual);
        call_args.push(arg);
    }

    let setup: Vec<syn::Stmt> = plan
        .args
        .iter()
        .map(|plan| {
            let storage = &plan.storage;
            let lvalue = &plan.lvalue;
            syn::parse_quote! {
                let #storage = std::sync::Arc::new(std::sync::Mutex::new(std::mem::take(&mut #lvalue)));
            }
        })
        .collect();
    let writeback: Vec<syn::Stmt> = plan
        .args
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

fn writeback_args(args: &[ast::Expr], param_types: &[typeinfer::GoType]) -> Vec<WritebackArg> {
    args.iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            let lvalue = call_arg_lvalue(arg, param_types.get(index))?;
            Some(WritebackArg {
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

    fn main_call<'a>(file: &'a ast::File<'a>, index: usize) -> Option<&'a ast::CallExpr<'a>> {
        let ast::Decl::FuncDecl(func) = file
            .decls
            .iter()
            .find(|decl| matches!(decl, ast::Decl::FuncDecl(func) if func.name.name == "main"))?
        else {
            return None;
        };
        let ast::Stmt::ExprStmt(expr) = func.body.as_ref()?.list.get(index)? else {
            return None;
        };
        let ast::Expr::CallExpr(call) = &expr.x else {
            return None;
        };
        Some(call)
    }

    #[test]
    fn needs_writeback_for_addressable_slice_passed_to_any_parameter() {
        let parsed = parse_file(
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
        );
        assert!(parsed.is_ok(), "expected test fixture to parse");
        let Ok(file) = parsed else {
            return;
        };
        let mut env = typeinfer::TypeEnv::new();
        env.scan_file(&file);
        env.set_var(
            "values",
            typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Int)),
        );
        super::super::set_type_env(env);

        let takes_any_call = main_call(&file, 0);
        assert!(takes_any_call.is_some(), "expected first call expression");
        let Some(takes_any_call) = takes_any_call else {
            return;
        };
        let takes_int_call = main_call(&file, 1);
        assert!(takes_int_call.is_some(), "expected second call expression");
        let Some(takes_int_call) = takes_int_call else {
            return;
        };

        assert!(needs_writeback(takes_any_call));
        assert!(!needs_writeback(takes_int_call));

        super::super::set_type_env(typeinfer::TypeEnv::new());
    }
}
