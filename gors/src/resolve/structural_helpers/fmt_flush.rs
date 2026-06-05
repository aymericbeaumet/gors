use super::syn_helpers::{
    has_method, is_self_expr, type_is_vec_u8, type_path_ident_name,
    type_path_pointer_cell_inner_name,
};
use crate::generated_names::{FMT_FLUSH_HOOK, fmt_flush_hook_ident};

type NameSet = std::collections::BTreeSet<String>;

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    for plan in fmt_flush_plans(items) {
        if !has_method(items, &plan.receiver, FMT_FLUSH_HOOK) {
            items.insert(0, fmt_flush_impl(&plan));
        }
    }
}

#[derive(Clone)]
struct FmtFlushPlan {
    receiver: String,
    source_field: String,
    source_buffer_field: String,
    source_buffer_access: BufferAccess,
    destination_field: String,
    trigger_methods: NameSet,
}

#[derive(Clone, Copy)]
enum BufferAccess {
    Direct,
    PointerCell,
}

fn fmt_flush_plans(items: &[syn::Item]) -> Vec<FmtFlushPlan> {
    let structs = named_structs(items);
    let mut plans = Vec::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        if let Some(plan) = fmt_flush_plan_for_receiver(items, &structs, item_struct) {
            plans.push(plan);
        }
    }
    plans.sort_by(|left, right| left.receiver.cmp(&right.receiver));
    plans.dedup_by(|left, right| left.receiver == right.receiver);
    plans
}

fn named_structs(items: &[syn::Item]) -> std::collections::BTreeMap<String, &syn::ItemStruct> {
    items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item_struct) => Some((item_struct.ident.to_string(), item_struct)),
            _ => None,
        })
        .collect()
}

fn fmt_flush_plan_for_receiver(
    items: &[syn::Item],
    structs: &std::collections::BTreeMap<String, &syn::ItemStruct>,
    receiver_struct: &syn::ItemStruct,
) -> Option<FmtFlushPlan> {
    let receiver = receiver_struct.ident.to_string();
    let receiver_fields = named_fields(receiver_struct)?;
    for source in &receiver_fields {
        let Some(source_ty) = type_path_ident_name(&source.ty) else {
            continue;
        };
        let Some(source_struct) = structs.get(&source_ty) else {
            continue;
        };
        let Some((source_buffer_field, buffer_ty, source_buffer_access)) =
            source_buffer_field(source_struct, structs)
        else {
            continue;
        };
        let Some(destination) = receiver_fields
            .iter()
            .filter(|field| field.name != source.name)
            .find(|field| type_path_ident_name(&field.ty).as_deref() == Some(buffer_ty.as_str()))
        else {
            continue;
        };
        let source_methods = methods_touching_field_buffer(items, &source_ty, &source_buffer_field);
        if source_methods.is_empty() {
            continue;
        }
        let trigger_methods =
            flush_trigger_methods(items, &receiver, &source.name, &source_methods);
        if trigger_methods.is_empty() {
            continue;
        }
        return Some(FmtFlushPlan {
            receiver,
            source_field: source.name.clone(),
            source_buffer_field,
            source_buffer_access,
            destination_field: destination.name.clone(),
            trigger_methods,
        });
    }
    None
}

#[derive(Clone)]
struct NamedField {
    name: String,
    ty: syn::Type,
}

fn named_fields(item_struct: &syn::ItemStruct) -> Option<Vec<NamedField>> {
    let syn::Fields::Named(fields) = &item_struct.fields else {
        return None;
    };
    Some(
        fields
            .named
            .iter()
            .filter_map(|field| {
                Some(NamedField {
                    name: field.ident.as_ref()?.to_string(),
                    ty: field.ty.clone(),
                })
            })
            .collect(),
    )
}

fn source_buffer_field(
    item_struct: &syn::ItemStruct,
    structs: &std::collections::BTreeMap<String, &syn::ItemStruct>,
) -> Option<(String, String, BufferAccess)> {
    for field in named_fields(item_struct)? {
        if let Some(buffer_ty) = type_path_pointer_cell_inner_name(&field.ty)
            && structs
                .get(&buffer_ty)
                .is_some_and(|item_struct| is_byte_buffer_struct(item_struct))
        {
            return Some((field.name, buffer_ty, BufferAccess::PointerCell));
        }
        if let Some(buffer_ty) = type_path_ident_name(&field.ty)
            && structs
                .get(&buffer_ty)
                .is_some_and(|item_struct| is_byte_buffer_struct(item_struct))
        {
            return Some((field.name, buffer_ty, BufferAccess::Direct));
        }
    }
    None
}

fn methods_touching_field_buffer(items: &[syn::Item], receiver: &str, field_name: &str) -> NameSet {
    let mut methods = NameSet::new();
    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if type_path_ident_name(&item_impl.self_ty).as_deref() != Some(receiver) {
            continue;
        }
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            Some(func)
        }) {
            if method_touches_self_field(func, field_name) {
                methods.insert(func.sig.ident.to_string());
            }
        }
    }
    methods
}

fn method_touches_self_field(func: &syn::ImplItemFn, field_name: &str) -> bool {
    struct Finder<'a> {
        field_name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if expr_mentions_direct_self_field(&assign.left, self.field_name) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_assign(self, assign);
        }

        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if expr_mentions_direct_self_field(&call.receiver, self.field_name) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        field_name,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.found
}

fn flush_trigger_methods(
    items: &[syn::Item],
    receiver: &str,
    source_field: &str,
    source_methods: &NameSet,
) -> NameSet {
    let mut direct_methods = NameSet::new();
    let mut calls_by_method = std::collections::BTreeMap::<String, NameSet>::new();

    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if type_path_ident_name(&item_impl.self_ty).as_deref() != Some(receiver) {
            continue;
        }
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident != FMT_FLUSH_HOOK).then_some(func)
        }) {
            let name = func.sig.ident.to_string();
            calls_by_method
                .entry(name.clone())
                .or_default()
                .extend(self_method_calls(func));
            if method_calls_source_method(func, source_field, source_methods) {
                direct_methods.insert(name);
            }
        }
    }

    expand_transitive_methods(direct_methods, &calls_by_method)
}

fn method_calls_source_method(
    func: &syn::ImplItemFn,
    source_field: &str,
    source_methods: &NameSet,
) -> bool {
    struct Finder<'a> {
        source_field: &'a str,
        source_methods: &'a NameSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if self.source_methods.contains(&call.method.to_string())
                && expr_mentions_direct_self_field(&call.receiver, self.source_field)
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        source_field,
        source_methods,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.found
}

fn self_method_calls(func: &syn::ImplItemFn) -> NameSet {
    struct Finder {
        calls: NameSet,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if is_self_expr(&call.receiver) {
                self.calls.insert(call.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        calls: NameSet::new(),
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.calls
}

fn expand_transitive_methods(
    mut methods: NameSet,
    calls_by_method: &std::collections::BTreeMap<String, NameSet>,
) -> NameSet {
    loop {
        let mut changed = false;
        for (method, callees) in calls_by_method {
            if methods.contains(method) || !callees.iter().any(|callee| methods.contains(callee)) {
                continue;
            }
            methods.insert(method.clone());
            changed = true;
        }
        if !changed {
            break;
        }
    }
    methods
}

fn expr_mentions_direct_self_field(expr: &syn::Expr, field_name: &str) -> bool {
    struct Finder<'a> {
        field_name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_field(&mut self, field: &syn::ExprField) {
            if is_self_expr(&field.base)
                && let syn::Member::Named(member) = &field.member
                && member == self.field_name
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_field(self, field);
        }
    }

    let mut finder = Finder {
        field_name,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn is_byte_buffer_struct(item_struct: &syn::ItemStruct) -> bool {
    let syn::Fields::Unnamed(fields) = &item_struct.fields else {
        return false;
    };
    let mut fields = fields.unnamed.iter();
    let Some(field) = fields.next() else {
        return false;
    };
    fields.next().is_none() && type_is_vec_u8(&field.ty)
}

fn fmt_flush_impl(plan: &FmtFlushPlan) -> syn::Item {
    let hook = fmt_flush_hook_ident();
    let receiver = syn::Ident::new(&plan.receiver, proc_macro2::Span::mixed_site());
    let source_doc = syn::LitStr::new(
        &crate::generated_names::fmt_flush_source_doc(&plan.source_field),
        proc_macro2::Span::mixed_site(),
    );
    let trigger_docs = plan
        .trigger_methods
        .iter()
        .map(|method| {
            syn::LitStr::new(
                &crate::generated_names::fmt_flush_method_doc(method),
                proc_macro2::Span::mixed_site(),
            )
        })
        .collect::<Vec<_>>();
    let source_field = syn::Ident::new(&plan.source_field, proc_macro2::Span::mixed_site());
    let source_buffer_field =
        syn::Ident::new(&plan.source_buffer_field, proc_macro2::Span::mixed_site());
    let destination_field =
        syn::Ident::new(&plan.destination_field, proc_macro2::Span::mixed_site());
    let take_bytes: syn::Expr = match plan.source_buffer_access {
        BufferAccess::Direct => {
            syn::parse_quote! { std::mem::take(&mut self.#source_field.#source_buffer_field.0) }
        }
        BufferAccess::PointerCell => {
            syn::parse_quote! { std::mem::take(&mut self.#source_field.#source_buffer_field.lock().unwrap().0) }
        }
    };
    syn::parse_quote! {
        impl #receiver {
            #[doc = #source_doc]
            #(#[doc = #trigger_docs])*
            fn #hook(&mut self) {
                let bytes = #take_bytes;
                self.#destination_field.0.extend(bytes);
            }
        }
    }
}
