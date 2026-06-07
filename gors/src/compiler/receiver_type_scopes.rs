use std::collections::{BTreeMap, HashSet};

use super::{
    receiver_type_facts::{
        ReceiverFieldTypeMap, ReceiverTypeContext, ReceiverTypeMap, ReceiverTypeRef,
        method_receiver_type_from_expr, receiver_type_from_init_expr, receiver_type_from_type,
        top_level_item_field_types, top_level_item_return_types,
    },
    syn_inspect::{item_name, named_self_type, pat_ident_name},
};

pub(super) struct ProgramFacts {
    module_names: HashSet<String>,
    item_names: HashSet<String>,
    top_level_return_types: ReceiverTypeMap,
    top_level_field_types: ReceiverFieldTypeMap,
}

impl ProgramFacts {
    pub(super) fn collect(modules: &BTreeMap<String, super::CompiledModule>) -> Self {
        let module_items = modules
            .values()
            .map(|module| (module.mod_name.clone(), module.file.items.as_slice()))
            .collect::<Vec<_>>();
        let module_names = module_items
            .iter()
            .map(|(module_name, _)| module_name.clone())
            .collect::<HashSet<_>>();
        let item_names = module_items
            .iter()
            .flat_map(|(_, items)| items.iter().filter_map(item_name))
            .collect::<HashSet<_>>();
        let top_level_return_types = module_items
            .iter()
            .flat_map(|(_, items)| top_level_item_return_types(items, &module_names))
            .collect();
        let top_level_field_types = module_items
            .iter()
            .flat_map(|(_, items)| top_level_item_field_types(items, &module_names))
            .collect();

        Self {
            module_names,
            item_names,
            top_level_return_types,
            top_level_field_types,
        }
    }

    pub(super) fn module_names(&self) -> &HashSet<String> {
        &self.module_names
    }

    pub(super) fn tracker(&self, module_name: String) -> Tracker<'_> {
        Tracker::new(
            module_name,
            &self.module_names,
            &self.item_names,
            &self.top_level_return_types,
            &self.top_level_field_types,
        )
    }
}

pub(super) struct Tracker<'a> {
    module_name: String,
    module_names: &'a HashSet<String>,
    item_names: &'a HashSet<String>,
    top_level_return_types: &'a ReceiverTypeMap,
    top_level_field_types: &'a ReceiverFieldTypeMap,
    local_types: Vec<BTreeMap<String, ReceiverTypeRef>>,
    current_self_type: Option<ReceiverTypeRef>,
}

impl<'a> Tracker<'a> {
    pub(super) fn new(
        module_name: String,
        module_names: &'a HashSet<String>,
        item_names: &'a HashSet<String>,
        top_level_return_types: &'a ReceiverTypeMap,
        top_level_field_types: &'a ReceiverFieldTypeMap,
    ) -> Self {
        Self {
            module_name,
            module_names,
            item_names,
            top_level_return_types,
            top_level_field_types,
            local_types: vec![BTreeMap::new()],
            current_self_type: None,
        }
    }

    pub(super) fn module_name(&self) -> &str {
        &self.module_name
    }

    pub(super) fn push_scope(&mut self) {
        self.local_types.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.local_types.pop();
    }

    pub(super) fn enter_impl(&mut self, item_impl: &syn::ItemImpl) -> Option<ReceiverTypeRef> {
        let previous = self.current_self_type.clone();
        self.current_self_type = named_self_type(&item_impl.self_ty)
            .map(|name| ReceiverTypeRef::new(Some(self.module_name.clone()), name));
        previous
    }

    pub(super) fn restore_impl(&mut self, previous: Option<ReceiverTypeRef>) {
        self.current_self_type = previous;
    }

    pub(super) fn record_fn_arg(&mut self, arg: &syn::FnArg) {
        record_fn_arg_receiver_type(arg, self.module_names, &mut self.local_types);
    }

    pub(super) fn record_local(&mut self, local: &syn::Local) {
        let bindings = {
            let context = self.context();
            let mut bindings = Vec::new();
            if let Some(binding) = local_receiver_type_binding(local, &context) {
                bindings.push(binding);
            }
            bindings.extend(tuple_local_receiver_type_bindings(local, &context));
            bindings
        };

        for (name, receiver_type) in bindings {
            record_local_receiver_type(name, receiver_type, &mut self.local_types);
        }
    }

    pub(super) fn receiver_type_for_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
        let context = self.context();
        method_receiver_type_from_expr(expr, &context)
    }

    pub(super) fn receiver_type_for_type(&self, ty: &syn::Type) -> Option<ReceiverTypeRef> {
        receiver_type_from_type(ty, self.module_names)
    }

    pub(super) fn current_self_type(&self) -> Option<&ReceiverTypeRef> {
        self.current_self_type.as_ref()
    }

    fn context(&self) -> ReceiverTypeContext<'_> {
        ReceiverTypeContext {
            module_names: self.module_names,
            item_names: self.item_names,
            top_level_return_types: self.top_level_return_types,
            top_level_field_types: self.top_level_field_types,
            scopes: &self.local_types,
            current_self_type: self.current_self_type.as_ref(),
        }
    }
}

fn record_fn_arg_receiver_type(
    arg: &syn::FnArg,
    module_names: &HashSet<String>,
    scopes: &mut [BTreeMap<String, ReceiverTypeRef>],
) {
    let syn::FnArg::Typed(pat_type) = arg else {
        return;
    };
    let Some(name) = pat_ident_name(&pat_type.pat) else {
        return;
    };
    let Some(receiver_type) = receiver_type_from_type(&pat_type.ty, module_names) else {
        return;
    };
    record_local_receiver_type(name, receiver_type, scopes);
}

fn local_receiver_type_binding(
    local: &syn::Local,
    context: &ReceiverTypeContext<'_>,
) -> Option<(String, ReceiverTypeRef)> {
    let name = pat_ident_name(&local.pat)?;
    if let syn::Pat::Type(pat_type) = &local.pat
        && let Some(receiver_type) = receiver_type_from_type(&pat_type.ty, context.module_names)
    {
        return Some((name, receiver_type));
    }
    let init = local.init.as_ref()?;
    let receiver_type = method_receiver_type_from_expr(&init.expr, context).or_else(|| {
        receiver_type_from_init_expr(
            &init.expr,
            context.module_names,
            context.item_names,
            context.top_level_return_types,
        )
    })?;
    Some((name, receiver_type))
}

fn tuple_local_receiver_type_bindings(
    local: &syn::Local,
    context: &ReceiverTypeContext<'_>,
) -> Vec<(String, ReceiverTypeRef)> {
    let syn::Pat::Tuple(pat_tuple) = &local.pat else {
        return Vec::new();
    };
    let Some(init) = &local.init else {
        return Vec::new();
    };
    let Some(expr_tuple) = expr_tuple(&init.expr) else {
        return Vec::new();
    };
    if pat_tuple.elems.len() != expr_tuple.elems.len() {
        return Vec::new();
    }

    pat_tuple
        .elems
        .iter()
        .zip(expr_tuple.elems.iter())
        .filter_map(|(pat, expr)| {
            let name = pat_ident_name(pat)?;
            let receiver_type = method_receiver_type_from_expr(expr, context).or_else(|| {
                receiver_type_from_init_expr(
                    expr,
                    context.module_names,
                    context.item_names,
                    context.top_level_return_types,
                )
            })?;
            Some((name, receiver_type))
        })
        .collect()
}

fn expr_tuple(expr: &syn::Expr) -> Option<&syn::ExprTuple> {
    match expr {
        syn::Expr::Group(group) => expr_tuple(&group.expr),
        syn::Expr::Paren(paren) => expr_tuple(&paren.expr),
        syn::Expr::Tuple(tuple) => Some(tuple),
        _ => None,
    }
}

fn record_local_receiver_type(
    name: String,
    receiver_type: ReceiverTypeRef,
    scopes: &mut [BTreeMap<String, ReceiverTypeRef>],
) {
    if let Some(scope) = scopes.last_mut() {
        scope.insert(name, receiver_type);
    }
}
