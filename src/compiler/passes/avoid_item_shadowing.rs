use std::collections::{HashMap, HashSet};

use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    let item_names = collect_item_names(&file.items);
    if item_names.is_empty() {
        return;
    }
    RenameFunctions { item_names }.visit_file_mut(file);
}

fn collect_item_names(items: &[syn::Item]) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in items {
        match item {
            syn::Item::Const(item) => {
                names.insert(item.ident.to_string());
            }
            syn::Item::Enum(item) => {
                names.insert(item.ident.to_string());
            }
            syn::Item::Fn(item) => {
                names.insert(item.sig.ident.to_string());
            }
            syn::Item::Static(item) => {
                names.insert(item.ident.to_string());
            }
            syn::Item::Struct(item) => {
                names.insert(item.ident.to_string());
            }
            syn::Item::Trait(item) => {
                names.insert(item.ident.to_string());
            }
            syn::Item::Type(item) => {
                names.insert(item.ident.to_string());
            }
            _ => {}
        }
    }
    names
}

struct RenameFunctions {
    item_names: HashSet<String>,
}

impl VisitMut for RenameFunctions {
    fn visit_item_fn_mut(&mut self, item: &mut syn::ItemFn) {
        rename_fn_body(&self.item_names, &mut item.sig, &mut item.block);
    }

    fn visit_impl_item_fn_mut(&mut self, item: &mut syn::ImplItemFn) {
        rename_fn_body(&self.item_names, &mut item.sig, &mut item.block);
    }
}

fn rename_fn_body(item_names: &HashSet<String>, sig: &mut syn::Signature, block: &mut syn::Block) {
    let mut collector = LocalConflictCollector {
        item_names,
        names: HashSet::new(),
    };
    for input in &mut sig.inputs {
        collector.visit_fn_arg_mut(input);
    }
    collector.visit_block_mut(block);
    if collector.names.is_empty() {
        return;
    }

    let renames = collector
        .names
        .into_iter()
        .map(|name| {
            let mut candidate = format!("{name}__local");
            let mut i = 0usize;
            while item_names.contains(&candidate) {
                i += 1;
                candidate = format!("{name}__local_{i}");
            }
            (
                name,
                syn::Ident::new(&candidate, proc_macro2::Span::mixed_site()),
            )
        })
        .collect();

    let mut renamer = LocalRenamer { renames };
    for input in &mut sig.inputs {
        renamer.visit_fn_arg_mut(input);
    }
    renamer.visit_block_mut(block);
}

struct LocalConflictCollector<'a> {
    item_names: &'a HashSet<String>,
    names: HashSet<String>,
}

impl VisitMut for LocalConflictCollector<'_> {
    fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
        let name = pat.ident.to_string();
        if self.item_names.contains(&name) {
            self.names.insert(name);
        }
        visit_mut::visit_pat_ident_mut(self, pat);
    }
}

struct LocalRenamer {
    renames: HashMap<String, syn::Ident>,
}

impl VisitMut for LocalRenamer {
    fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
        if let Some(new_ident) = self.renames.get(&pat.ident.to_string()) {
            pat.ident = new_ident.clone();
        }
        visit_mut::visit_pat_ident_mut(self, pat);
    }

    fn visit_expr_path_mut(&mut self, expr: &mut syn::ExprPath) {
        if expr.qself.is_none()
            && expr.path.leading_colon.is_none()
            && expr.path.segments.len() == 1
        {
            if let Some(segment) = expr.path.segments.iter_mut().next() {
                if let Some(new_ident) = self.renames.get(&segment.ident.to_string()) {
                    segment.ident = new_ident.clone();
                }
            }
        }
        visit_mut::visit_expr_path_mut(self, expr);
    }
}
