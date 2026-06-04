pub(super) type NameSet = std::collections::HashSet<String>;

pub(super) fn stmt_bound_names(stmt: &syn::Stmt) -> NameSet {
    let mut names = NameSet::new();
    if let syn::Stmt::Local(local) = stmt {
        collect_pat_names(&local.pat, &mut names);
    }
    names
}

fn collect_pat_names(pat: &syn::Pat, names: &mut NameSet) {
    match pat {
        syn::Pat::Ident(pat_ident) => {
            names.insert(pat_ident.ident.to_string());
        }
        syn::Pat::Or(pat_or) => {
            for case in &pat_or.cases {
                collect_pat_names(case, names);
            }
        }
        syn::Pat::Paren(paren) => collect_pat_names(&paren.pat, names),
        syn::Pat::Reference(reference) => collect_pat_names(&reference.pat, names),
        syn::Pat::Rest(_) => {}
        syn::Pat::Slice(slice) => {
            for elem in &slice.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::Struct(pat_struct) => {
            for field in &pat_struct.fields {
                collect_pat_names(&field.pat, names);
            }
        }
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::TupleStruct(tuple_struct) => {
            for elem in &tuple_struct.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::Type(pat_type) => collect_pat_names(&pat_type.pat, names),
        syn::Pat::Wild(_) => {}
        _ => {}
    }
}

pub(super) fn stmt_mentions_any_name(stmt: &syn::Stmt, names: &NameSet) -> bool {
    if names.is_empty() {
        return false;
    }
    match stmt {
        syn::Stmt::Expr(expr, _) => expr_mentions_any_name(expr, names),
        syn::Stmt::Local(local) => local
            .init
            .as_ref()
            .is_some_and(|init| expr_mentions_any_name(&init.expr, names)),
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => false,
    }
}

pub(super) fn expr_mentions_any_name(expr: &syn::Expr, names: &NameSet) -> bool {
    if names.is_empty() {
        return false;
    }

    struct Visitor<'a> {
        names: &'a NameSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Visitor<'_> {
        fn visit_expr_path(&mut self, expr_path: &syn::ExprPath) {
            if expr_path.path.leading_colon.is_none()
                && expr_path.path.segments.len() == 1
                && expr_path
                    .path
                    .segments
                    .first()
                    .is_some_and(|seg| self.names.contains(&seg.ident.to_string()))
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_path(self, expr_path);
        }
    }

    let mut visitor = Visitor {
        names,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut visitor, expr);
    visitor.found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_bound_local_names_and_method_call_argument_uses() {
        let local: syn::Stmt = syn::parse_quote! {
            let mut fallback = self.value;
        };
        let names = stmt_bound_names(&local);
        assert!(names.contains("fallback"), "expected local name: {names:?}");

        let call: syn::Stmt = syn::parse_quote! {
            self.printValue(fallback);
        };
        assert!(
            stmt_mentions_any_name(&call, &names),
            "expected method-call argument to mention dropped local"
        );
    }

    #[test]
    fn tracks_destructured_local_names() {
        let local: syn::Stmt = syn::parse_quote! {
            let (left, Some(right)): (isize, Option<isize>) = value;
        };
        let names = stmt_bound_names(&local);

        assert!(names.contains("left"), "{names:?}");
        assert!(names.contains("right"), "{names:?}");
        assert!(!names.contains("value"), "{names:?}");
    }
}
