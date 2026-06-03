use super::{
    ImplSelfType, has_impl, has_method, type_path_ident_name, type_path_pointer_cell_inner_name,
};

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    for plan in fmt_flush_plans(items) {
        if !has_method(items, &plan.receiver, "__gors_flush_fmt") {
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
    if !has_state_impl_for_receiver(items, &receiver) {
        return None;
    }
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
        return Some(FmtFlushPlan {
            receiver,
            source_field: source.name.clone(),
            source_buffer_field,
            source_buffer_access,
            destination_field: destination.name.clone(),
        });
    }
    None
}

fn has_state_impl_for_receiver(items: &[syn::Item], receiver: &str) -> bool {
    has_impl(items, "State", ImplSelfType::Named(receiver))
        || has_impl(items, "State", ImplSelfType::PointerCellToNamed(receiver))
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

fn type_is_vec_u8(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() {
        return false;
    }
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    let mut args = arguments.args.iter();
    let Some(syn::GenericArgument::Type(inner)) = args.next() else {
        return false;
    };
    args.next().is_none() && type_path_ident_name(inner).as_deref() == Some("u8")
}

fn fmt_flush_impl(plan: &FmtFlushPlan) -> syn::Item {
    let receiver = syn::Ident::new(&plan.receiver, proc_macro2::Span::mixed_site());
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
            fn __gors_flush_fmt(&mut self) {
                let bytes = #take_bytes;
                self.#destination_field.0.extend(bytes);
            }
        }
    }
}
