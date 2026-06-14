type FieldSet = std::collections::HashSet<String>;

use crate::compiler::syn_inspect::is_self_expr;

pub(super) fn expr_mentions(expr: &syn::Expr, fields: &FieldSet) -> bool {
    let mut finder = Finder {
        fields,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

pub(super) fn is_self_field_in(field: &syn::ExprField, fields: &FieldSet) -> bool {
    is_self_expr(&field.base)
        && member_ident_name(&field.member)
            .is_some_and(|member| fields.contains(&member.to_string()))
}

struct Finder<'a> {
    fields: &'a FieldSet,
    found: bool,
}

impl syn::visit::Visit<'_> for Finder<'_> {
    fn visit_expr_field(&mut self, field: &syn::ExprField) {
        if is_self_field_in(field, self.fields) {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn member_ident_name(member: &syn::Member) -> Option<&syn::Ident> {
    match member {
        syn::Member::Named(ident) => Some(ident),
        syn::Member::Unnamed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_named_self_fields_inside_expressions() {
        let fields = FieldSet::from(["value".to_string()]);
        let expr: syn::Expr = syn::parse_quote! {
            self.value.Type()
        };

        assert!(expr_mentions(&expr, &fields));
    }
}
