pub(super) use super::super::super::syn_inspect::{
    is_deref_self_expr, is_path_ident, is_self_expr, is_self_or_deref_self_expr, path_ident_name,
    strip_paren_or_group, type_path_ident_name,
};

pub(super) fn is_path_call(func: &syn::Expr, segments: &[&str]) -> bool {
    super::super::super::syn_inspect::is_path_call_expr(func, segments)
}
