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

    let mut visitor = NameUseVisitor {
        names,
        shadowed: std::collections::BTreeMap::new(),
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut visitor, expr);
    visitor.found
}

struct NameUseVisitor<'a> {
    names: &'a NameSet,
    shadowed: std::collections::BTreeMap<String, usize>,
    found: bool,
}

impl NameUseVisitor<'_> {
    fn is_visible_target(&self, ident: &syn::Ident) -> bool {
        let name = ident.to_string();
        self.names.contains(&name) && !self.shadowed.contains_key(&name)
    }

    fn push_shadowed(&mut self, names: &NameSet) -> Vec<String> {
        names
            .iter()
            .filter(|name| self.names.contains(*name))
            .map(|name| {
                *self.shadowed.entry(name.clone()).or_default() += 1;
                name.clone()
            })
            .collect()
    }

    fn pop_shadowed(&mut self, pushed: Vec<String>) {
        for name in pushed {
            let Some(count) = self.shadowed.get_mut(&name) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                self.shadowed.remove(&name);
            }
        }
    }

    fn visit_stmt_with_shadowed(&mut self, stmt: &syn::Stmt, shadowed: &NameSet) {
        let pushed = self.push_shadowed(shadowed);
        match stmt {
            syn::Stmt::Expr(expr, _) => syn::visit::Visit::visit_expr(self, expr),
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    syn::visit::Visit::visit_expr(self, &init.expr);
                }
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
        self.pop_shadowed(pushed);
    }
}

impl syn::visit::Visit<'_> for NameUseVisitor<'_> {
    fn visit_block(&mut self, block: &syn::Block) {
        let mut shadowed = NameSet::new();
        for stmt in &block.stmts {
            self.visit_stmt_with_shadowed(stmt, &shadowed);
            if self.found {
                return;
            }
            for name in stmt_bound_names(stmt) {
                if self.names.contains(&name) {
                    shadowed.insert(name);
                }
            }
        }
    }

    fn visit_arm(&mut self, arm: &syn::Arm) {
        let mut shadowed = NameSet::new();
        collect_pat_names(&arm.pat, &mut shadowed);
        let pushed = self.push_shadowed(&shadowed);
        if let Some((_, guard)) = &arm.guard {
            syn::visit::Visit::visit_expr(self, guard);
        }
        if !self.found {
            syn::visit::Visit::visit_expr(self, &arm.body);
        }
        self.pop_shadowed(pushed);
    }

    fn visit_expr_closure(&mut self, closure: &syn::ExprClosure) {
        let mut shadowed = NameSet::new();
        for input in &closure.inputs {
            collect_pat_names(input, &mut shadowed);
        }
        let pushed = self.push_shadowed(&shadowed);
        syn::visit::Visit::visit_expr(self, &closure.body);
        self.pop_shadowed(pushed);
    }

    fn visit_expr_for_loop(&mut self, for_loop: &syn::ExprForLoop) {
        syn::visit::Visit::visit_expr(self, &for_loop.expr);
        if self.found {
            return;
        }
        let mut shadowed = NameSet::new();
        collect_pat_names(&for_loop.pat, &mut shadowed);
        let pushed = self.push_shadowed(&shadowed);
        syn::visit::Visit::visit_block(self, &for_loop.body);
        self.pop_shadowed(pushed);
    }

    fn visit_expr_path(&mut self, expr_path: &syn::ExprPath) {
        if expr_path.path.leading_colon.is_none()
            && expr_path.path.segments.len() == 1
            && expr_path
                .path
                .segments
                .first()
                .is_some_and(|seg| self.is_visible_target(&seg.ident))
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_path(self, expr_path);
    }
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

    #[test]
    fn ignores_shadowed_names_inside_nested_blocks() {
        let names = NameSet::from(["fallback".to_string()]);
        let block: syn::Stmt = syn::parse_quote! {
            {
                let fallback = 1;
                self.printValue(fallback);
            }
        };

        assert!(
            !stmt_mentions_any_name(&block, &names),
            "expected shadowed fallback local not to count as outer use"
        );
    }

    #[test]
    fn detects_outer_names_before_nested_shadowing() {
        let names = NameSet::from(["fallback".to_string()]);
        let block: syn::Stmt = syn::parse_quote! {
            {
                self.printValue(fallback);
                let fallback = 1;
                self.printValue(fallback);
            }
        };

        assert!(
            stmt_mentions_any_name(&block, &names),
            "expected use before shadowing to count as outer use"
        );
    }

    #[test]
    fn detects_outer_names_in_shadowing_initializers() {
        let names = NameSet::from(["fallback".to_string()]);
        let block: syn::Stmt = syn::parse_quote! {
            {
                let fallback = fallback;
                self.printValue(fallback);
            }
        };

        assert!(
            stmt_mentions_any_name(&block, &names),
            "expected initializer to see outer name before local binding shadows it"
        );
    }

    #[test]
    fn closure_parameters_shadow_outer_names() {
        let names = NameSet::from(["fallback".to_string()]);
        let closure: syn::Stmt = syn::parse_quote! {
            let f = |fallback| fallback + 1;
        };

        assert!(
            !stmt_mentions_any_name(&closure, &names),
            "expected closure parameter to shadow outer name"
        );
    }
}
