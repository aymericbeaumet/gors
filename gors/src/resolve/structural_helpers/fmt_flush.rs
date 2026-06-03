use super::{ImplSelfType, has_impl, has_method};

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    for receiver in fmt_flush_receivers(items) {
        if !has_method(items, &receiver, "__gors_flush_fmt") {
            items.insert(0, fmt_flush_impl(&receiver));
        }
    }
}

fn fmt_flush_receivers(items: &[syn::Item]) -> Vec<String> {
    let mut receivers = std::collections::BTreeSet::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let receiver = item_struct.ident.to_string();
        if struct_has_fmt_flush_fields(item_struct) && has_state_impl_for_receiver(items, &receiver)
        {
            receivers.insert(receiver);
        }
    }
    receivers.into_iter().collect()
}

fn has_state_impl_for_receiver(items: &[syn::Item], receiver: &str) -> bool {
    has_impl(items, "State", ImplSelfType::Named(receiver))
        || has_impl(items, "State", ImplSelfType::PointerCellToNamed(receiver))
}

fn struct_has_fmt_flush_fields(item_struct: &syn::ItemStruct) -> bool {
    let syn::Fields::Named(fields) = &item_struct.fields else {
        return false;
    };
    let mut has_fmt = false;
    let mut has_buf = false;
    for field in &fields.named {
        let Some(name) = field.ident.as_ref().map(ToString::to_string) else {
            continue;
        };
        has_fmt |= name == "fmt";
        has_buf |= name == "buf";
    }
    has_fmt && has_buf
}

fn fmt_flush_impl(receiver: &str) -> syn::Item {
    let receiver = syn::Ident::new(receiver, proc_macro2::Span::mixed_site());
    syn::parse_quote! {
        impl #receiver {
            fn __gors_flush_fmt(&mut self) {
                let bytes = std::mem::take(&mut self.fmt.buf.lock().unwrap().0);
                self.buf.0.extend(bytes);
            }
        }
    }
}
