use std::collections::{BTreeMap, HashSet};

use super::{
    ReceiverFieldTypeMap, ReceiverTypeContext, ReceiverTypeMap, ReceiverTypeRef,
    local_receiver_type_binding, method_receiver_type_from_expr, record_fn_arg_receiver_type,
    record_local_receiver_type, syn_inspect::named_self_type,
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
