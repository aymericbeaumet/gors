use std::collections::{BTreeMap, HashSet};

use super::{
    receiver_type_facts::{
        ReceiverFieldTypeMap, ReceiverTypeContext, ReceiverTypeMap, ReceiverTypeRef,
        method_receiver_type_from_expr, receiver_type_from_init_expr, receiver_type_from_type,
    },
    syn_inspect::{named_self_type, pat_ident_name},
};

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
        self.current_self_type = named_self_type(&item_impl.self_ty).map(|name| ReceiverTypeRef {
            module: Some(self.module_name.clone()),
            name,
        });
        previous
    }

    pub(super) fn restore_impl(&mut self, previous: Option<ReceiverTypeRef>) {
        self.current_self_type = previous;
    }

    pub(super) fn record_fn_arg(&mut self, arg: &syn::FnArg) {
        record_fn_arg_receiver_type(arg, self.module_names, &mut self.local_types);
    }

    pub(super) fn record_local(&mut self, local: &syn::Local) {
        let context = self.context();
        if let Some((name, receiver_type)) = local_receiver_type_binding(local, &context) {
            record_local_receiver_type(name, receiver_type, &mut self.local_types);
        }
    }

    pub(super) fn receiver_type_for_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
        let context = self.context();
        method_receiver_type_from_expr(expr, &context)
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

fn record_local_receiver_type(
    name: String,
    receiver_type: ReceiverTypeRef,
    scopes: &mut [BTreeMap<String, ReceiverTypeRef>],
) {
    if let Some(scope) = scopes.last_mut() {
        scope.insert(name, receiver_type);
    }
}
