use proc_macro2::Span;
use std::cell::RefCell;

thread_local! {
    static DEFER_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SWITCH_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SELECT_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static GOTO_STATE_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static RANGE_FUNCTION_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static NAMED_RETURN_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static UNNAMED_ARG_COUNTER: RefCell<usize> = const { RefCell::new(0) };
}

pub(super) fn reset_lowering_counters() {
    DEFER_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    SWITCH_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    SELECT_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    RANGE_FUNCTION_COUNTER.with(|counter| *counter.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|counter| *counter.borrow_mut() = 0);
}

pub(super) fn reset_unnamed_arg_counter() {
    UNNAMED_ARG_COUNTER.with(|counter| *counter.borrow_mut() = 0);
}

pub(super) fn next_unnamed_arg_ident() -> syn::Ident {
    let n = next_unnamed_arg_id();
    syn::Ident::new(&format!("__gors_arg_{n}"), Span::mixed_site())
}

pub(super) fn next_defer_id() -> usize {
    next_id(&DEFER_COUNTER)
}

pub(super) fn next_switch_label() -> syn::Lifetime {
    let n = next_id(&SWITCH_COUNTER);
    syn::Lifetime::new(&format!("'__gors_switch_{n}"), Span::mixed_site())
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

pub(super) fn next_range_function_id() -> usize {
    next_id(&RANGE_FUNCTION_COUNTER)
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
            next_type_switch_value_ident().to_string(),
            "__gors_type_switch_value_1"
        );
        assert_eq!(next_select_label().ident.to_string(), "__gors_select_0");
        assert_eq!(next_defer_id(), 0);
        assert_eq!(next_range_function_id(), 0);
        assert_eq!(
            next_named_return_label().ident.to_string(),
            "__gors_named_return_0"
        );
        let mut named_return_temps = next_named_return_temp_idents(2).into_iter();
        assert_eq!(
            named_return_temps.next().map(|ident| ident.to_string()),
            Some("__gors_named_return_1_0".to_string())
        );
        assert_eq!(next_goto_state_names().0.to_string(), "__gors_goto_state_0");

        reset_lowering_counters();
        assert_eq!(next_switch_label().ident.to_string(), "__gors_switch_0");
        assert_eq!(
            next_select_recv_idents().0.to_string(),
            "__gors_select_value_0"
        );
        assert_eq!(next_defer_id(), 0);
        assert_eq!(next_range_function_id(), 0);
        assert_eq!(
            next_named_return_label().ident.to_string(),
            "__gors_named_return_0"
        );
    }

    #[test]
    fn unnamed_arg_counter_resets_independently() {
        reset_unnamed_arg_counter();
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_0");
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_1");

        reset_unnamed_arg_counter();
        assert_eq!(next_unnamed_arg_ident().to_string(), "__gors_arg_0");
    }
}
