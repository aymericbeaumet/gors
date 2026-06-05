use proc_macro2::Span;
use std::cell::RefCell;

use super::import_context::import_rust_name;

thread_local! {
    static DEFER_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SWITCH_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SELECT_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static GOTO_STATE_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static RANGE_FUNCTION_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static NAMED_RETURN_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static LOOP_BODY_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static UNNAMED_ARG_COUNTER: RefCell<usize> = const { RefCell::new(0) };
}

pub(super) fn reset_lowering_counters() {
    DEFER_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    SWITCH_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    SELECT_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    RANGE_FUNCTION_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    LOOP_BODY_COUNTER.with(|counter| *counter.borrow_mut() = 0);
}

pub(super) fn reset_unnamed_arg_counter() {
    UNNAMED_ARG_COUNTER.with(|counter| *counter.borrow_mut() = 0);
}

pub(super) fn next_unnamed_arg_ident() -> syn::Ident {
    let n = next_unnamed_arg_id();
    unnamed_arg_ident(n)
}

pub(super) fn unnamed_arg_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_arg_{index}"), Span::mixed_site())
}

pub(super) fn next_defer_id() -> usize {
    next_id(&DEFER_COUNTER)
}

pub(super) fn defer_arg_temp_ident(defer_id: usize, arg_index: usize) -> syn::Ident {
    syn::Ident::new(
        &format!("_defer_{defer_id}_arg_{arg_index}"),
        Span::mixed_site(),
    )
}

pub(super) fn defer_fun_temp_ident(defer_id: usize) -> syn::Ident {
    syn::Ident::new(&format!("_defer_{defer_id}_fun"), Span::mixed_site())
}

pub(super) fn borrowed_interface_lifetime() -> syn::Lifetime {
    syn::Lifetime::new("'__gors", Span::mixed_site())
}

pub(super) fn borrowed_interface_lifetime_param() -> syn::GenericParam {
    syn::GenericParam::Lifetime(syn::LifetimeParam::new(borrowed_interface_lifetime()))
}

pub(super) fn next_switch_label() -> syn::Lifetime {
    let n = next_id(&SWITCH_COUNTER);
    syn::Lifetime::new(&format!("'__gors_switch_{n}"), Span::mixed_site())
}

pub(super) fn switch_fallthrough_ident() -> syn::Ident {
    syn::Ident::new("__gors_switch_fallthrough", Span::mixed_site())
}

pub(super) fn switch_selected_ident() -> syn::Ident {
    syn::Ident::new("__gors_switch_selected", Span::mixed_site())
}

pub(super) fn switch_tag_ident() -> syn::Ident {
    syn::Ident::new("__gors_switch_tag", Span::mixed_site())
}

pub(super) fn comma_ok_value_ident() -> syn::Ident {
    syn::Ident::new("__gors_comma_ok_value", Span::mixed_site())
}

pub(super) fn comma_ok_ok_ident() -> syn::Ident {
    syn::Ident::new("__gors_comma_ok_ok", Span::mixed_site())
}

pub(super) fn multi_value_temp_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_multi_{index}"), Span::mixed_site())
}

pub(super) fn assignment_temp_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_assign_{index}"), Span::mixed_site())
}

pub(super) fn shared_value_ident() -> syn::Ident {
    syn::Ident::new("__gors_shared_value", Span::mixed_site())
}

pub(super) fn string_const_bytes_fn_ident(name: &str) -> syn::Ident {
    syn::Ident::new(
        &format!("__gors_string_const_bytes_{}", import_rust_name(name)),
        Span::mixed_site(),
    )
}

pub(super) fn string_bytes_temp_ident() -> syn::Ident {
    syn::Ident::new("__gors_string_bytes", Span::mixed_site())
}

pub(super) fn preborrow_arg_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_preborrow_arg_{index}"), Span::mixed_site())
}

pub(super) fn premethod_arg_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_premethod_arg_{index}"), Span::mixed_site())
}

pub(super) fn vec_newtype_receiver_temp_ident(index: usize) -> syn::Ident {
    syn::Ident::new(
        &format!("__gors_vec_newtype_recv_{index}"),
        Span::mixed_site(),
    )
}

pub(super) fn vec_newtype_arg_temp_ident(index: usize) -> syn::Ident {
    syn::Ident::new(
        &format!("__gors_vec_newtype_arg_{index}"),
        Span::mixed_site(),
    )
}

pub(super) fn slice_base_index_ident() -> syn::Ident {
    syn::Ident::new("__gors_slice_base_index", Span::mixed_site())
}

pub(super) fn slice_alias_offset_ident() -> syn::Ident {
    syn::Ident::new("__gors_slice_alias_offset", Span::mixed_site())
}

pub(super) fn slice_alias_index_ident() -> syn::Ident {
    syn::Ident::new("__gors_slice_alias_index", Span::mixed_site())
}

pub(super) fn slice_alias_value_ident() -> syn::Ident {
    syn::Ident::new("__gors_slice_alias_value", Span::mixed_site())
}

pub(super) fn next_type_switch_value_ident() -> syn::Ident {
    let n = next_id(&SWITCH_COUNTER);
    syn::Ident::new(&format!("__gors_type_switch_value_{n}"), Span::mixed_site())
}

pub(super) fn next_select_label() -> syn::Lifetime {
    let n = next_id(&SELECT_COUNTER);
    syn::Lifetime::new(&format!("'__gors_select_{n}"), Span::mixed_site())
}

pub(super) fn next_select_recv_idents() -> (syn::Ident, syn::Ident) {
    let n = next_id(&SELECT_COUNTER);
    (
        syn::Ident::new(&format!("__gors_select_value_{n}"), Span::mixed_site()),
        syn::Ident::new(&format!("__gors_select_ok_{n}"), Span::mixed_site()),
    )
}

pub(super) fn next_goto_state_names() -> (syn::Ident, syn::Lifetime) {
    let n = next_id(&GOTO_STATE_COUNTER);
    (
        syn::Ident::new(&format!("__gors_goto_state_{n}"), Span::mixed_site()),
        syn::Lifetime::new(&format!("'__gors_goto_{n}"), Span::mixed_site()),
    )
}

pub(super) fn range_assign_temp_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_range_{index}"), Span::mixed_site())
}

pub(super) fn range_function_arg_ident(index: usize) -> syn::Ident {
    syn::Ident::new(&format!("__gors_range_arg_{index}"), Span::mixed_site())
}

pub(super) fn next_range_function_return_idents() -> (syn::Ident, syn::Ident) {
    let n = next_id(&RANGE_FUNCTION_COUNTER);
    (
        syn::Ident::new(&format!("__gors_range_return_{n}"), Span::mixed_site()),
        syn::Ident::new(
            &format!("__gors_range_return_for_yield_{n}"),
            Span::mixed_site(),
        ),
    )
}

pub(super) fn method_receiver_ident() -> syn::Ident {
    syn::Ident::new("__gors_method_receiver", Span::mixed_site())
}

pub(super) fn method_arg_idents(count: usize) -> Vec<syn::Ident> {
    (0..count)
        .map(|index| syn::Ident::new(&format!("__gors_method_arg_{index}"), Span::mixed_site()))
        .collect()
}

pub(super) fn next_named_return_label() -> syn::Lifetime {
    let n = next_named_return_id();
    syn::Lifetime::new(&format!("'__gors_named_return_{n}"), Span::mixed_site())
}

pub(super) fn next_named_return_temp_idents(count: usize) -> Vec<syn::Ident> {
    let n = next_named_return_id();
    (0..count)
        .map(|idx| {
            syn::Ident::new(
                &format!("__gors_named_return_{n}_{idx}"),
                Span::mixed_site(),
            )
        })
        .collect()
}

fn next_named_return_id() -> usize {
    next_id(&NAMED_RETURN_COUNTER)
}

pub(super) fn next_loop_body_label() -> syn::Lifetime {
    let n = next_id(&LOOP_BODY_COUNTER);
    syn::Lifetime::new(&format!("'__gors_loop_body_{n}"), Span::mixed_site())
}

fn next_unnamed_arg_id() -> usize {
    next_id(&UNNAMED_ARG_COUNTER)
}

fn next_id(key: &'static std::thread::LocalKey<RefCell<usize>>) -> usize {
    key.with(|counter| {
        let mut counter = counter.borrow_mut();
        let id = *counter;
        *counter += 1;
        id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_counters_reset_all_control_name_sequences() {
        reset_lowering_counters();
        assert_eq!(next_switch_label().ident.to_string(), "__gors_switch_0");
        assert_eq!(
            switch_fallthrough_ident().to_string(),
            "__gors_switch_fallthrough"
        );
        assert_eq!(
            switch_selected_ident().to_string(),
            "__gors_switch_selected"
        );
        assert_eq!(switch_tag_ident().to_string(), "__gors_switch_tag");
        assert_eq!(defer_arg_temp_ident(1, 2).to_string(), "_defer_1_arg_2");
        assert_eq!(defer_fun_temp_ident(3).to_string(), "_defer_3_fun");
        assert_eq!(borrowed_interface_lifetime().ident.to_string(), "__gors");
        assert!(
            matches!(
                borrowed_interface_lifetime_param(),
                syn::GenericParam::Lifetime(_)
            ),
            "expected borrowed interface helper to produce a lifetime generic parameter"
        );
        assert_eq!(comma_ok_value_ident().to_string(), "__gors_comma_ok_value");
        assert_eq!(comma_ok_ok_ident().to_string(), "__gors_comma_ok_ok");
        assert_eq!(multi_value_temp_ident(4).to_string(), "__gors_multi_4");
        assert_eq!(assignment_temp_ident(5).to_string(), "__gors_assign_5");
        assert_eq!(shared_value_ident().to_string(), "__gors_shared_value");
        assert_eq!(
            string_const_bytes_fn_ident("type").to_string(),
            "__gors_string_const_bytes_type_"
        );
        assert_eq!(string_bytes_temp_ident().to_string(), "__gors_string_bytes");
        assert_eq!(preborrow_arg_ident(6).to_string(), "__gors_preborrow_arg_6");
        assert_eq!(premethod_arg_ident(7).to_string(), "__gors_premethod_arg_7");
        assert_eq!(
            vec_newtype_receiver_temp_ident(8).to_string(),
            "__gors_vec_newtype_recv_8"
        );
        assert_eq!(
            vec_newtype_arg_temp_ident(9).to_string(),
            "__gors_vec_newtype_arg_9"
        );
        assert_eq!(
            slice_base_index_ident().to_string(),
            "__gors_slice_base_index"
        );
        assert_eq!(
            slice_alias_offset_ident().to_string(),
            "__gors_slice_alias_offset"
        );
        assert_eq!(
            slice_alias_index_ident().to_string(),
            "__gors_slice_alias_index"
        );
        assert_eq!(
            slice_alias_value_ident().to_string(),
            "__gors_slice_alias_value"
        );
        assert_eq!(
            next_type_switch_value_ident().to_string(),
            "__gors_type_switch_value_1"
        );
        assert_eq!(next_select_label().ident.to_string(), "__gors_select_0");
        assert_eq!(next_defer_id(), 0);
        assert_eq!(range_assign_temp_ident(2).to_string(), "__gors_range_2");
        assert_eq!(
            range_function_arg_ident(3).to_string(),
            "__gors_range_arg_3"
        );
        let (range_return, range_return_for_yield) = next_range_function_return_idents();
        assert_eq!(range_return.to_string(), "__gors_range_return_0");
        assert_eq!(
            range_return_for_yield.to_string(),
            "__gors_range_return_for_yield_0"
        );
        assert_eq!(
            method_receiver_ident().to_string(),
            "__gors_method_receiver"
        );
        let mut method_args = method_arg_idents(2).into_iter();
        assert_eq!(
            method_args.next().map(|ident| ident.to_string()),
            Some("__gors_method_arg_0".to_string())
        );
        assert_eq!(
            next_named_return_label().ident.to_string(),
            "__gors_named_return_0"
        );
        let mut named_return_temps = next_named_return_temp_idents(2).into_iter();
        assert_eq!(
            named_return_temps.next().map(|ident| ident.to_string()),
            Some("__gors_named_return_1_0".to_string())
        );
        assert_eq!(
            next_loop_body_label().ident.to_string(),
            "__gors_loop_body_0"
        );
        assert_eq!(next_goto_state_names().0.to_string(), "__gors_goto_state_0");

        reset_lowering_counters();
        assert_eq!(next_switch_label().ident.to_string(), "__gors_switch_0");
        assert_eq!(
            next_select_recv_idents().0.to_string(),
            "__gors_select_value_0"
        );
        assert_eq!(next_defer_id(), 0);
        assert_eq!(
            next_range_function_return_idents().0.to_string(),
            "__gors_range_return_0"
        );
        assert_eq!(
            next_named_return_label().ident.to_string(),
            "__gors_named_return_0"
        );
        assert_eq!(
            next_loop_body_label().ident.to_string(),
            "__gors_loop_body_0"
        );
    }

    #[test]
    fn unnamed_arg_counter_resets_independently() {
        reset_unnamed_arg_counter();
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_0");
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_1");
        assert_eq!(unnamed_arg_ident(7).to_string(), "__gors_arg_7");

        reset_unnamed_arg_counter();
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_0");
    }
}
