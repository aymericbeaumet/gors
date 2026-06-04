use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use proc_macro2::Span;

use super::rust_safe_ident_name;

thread_local! {
    static GOTO_CONTINUE_LABELS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static GOTO_STATE_CONTEXTS: RefCell<Vec<GotoStateContext>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct GotoContinueLabelsGuard {
    previous: HashSet<String>,
}

#[derive(Clone)]
pub(super) struct GotoStateContext {
    state_ident: syn::Ident,
    loop_label: syn::Lifetime,
    labels: BTreeMap<String, usize>,
}

pub(super) struct GotoStateContextGuard;

impl GotoContinueLabelsGuard {
    pub(super) fn extend(names: impl IntoIterator<Item = String>) -> Self {
        let previous = GOTO_CONTINUE_LABELS.with(|labels| {
            let previous = labels.borrow().clone();
            labels.borrow_mut().extend(names);
            previous
        });
        Self { previous }
    }
}

impl Drop for GotoContinueLabelsGuard {
    fn drop(&mut self) {
        GOTO_CONTINUE_LABELS.with(|labels| {
            *labels.borrow_mut() = self.previous.clone();
        });
    }
}

impl GotoStateContext {
    pub(super) fn new(
        state_ident: syn::Ident,
        loop_label: syn::Lifetime,
        labels: BTreeMap<String, usize>,
    ) -> Self {
        Self {
            state_ident,
            loop_label,
            labels,
        }
    }
}

impl GotoStateContextGuard {
    pub(super) fn push(context: GotoStateContext) -> Self {
        GOTO_STATE_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().push(context);
        });
        Self
    }
}

impl Drop for GotoStateContextGuard {
    fn drop(&mut self) {
        GOTO_STATE_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().pop();
        });
    }
}

pub(super) fn clear_state_contexts() {
    GOTO_STATE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
}

pub(super) fn is_continue_label(name: &str) -> bool {
    GOTO_CONTINUE_LABELS.with(|labels| labels.borrow().contains(name))
}

pub(super) fn compile_state_jump(name: &str) -> Option<Vec<syn::Stmt>> {
    let target = rust_safe_ident_name(name);
    GOTO_STATE_CONTEXTS.with(|contexts| {
        contexts.borrow().iter().rev().find_map(|context| {
            context.labels.get(&target).map(|target_index| {
                let state_ident = &context.state_ident;
                let loop_label = &context.loop_label;
                let target_lit =
                    syn::LitInt::new(&format!("{target_index}usize"), Span::mixed_site());
                vec![
                    syn::parse_quote! {
                        #state_ident = #target_lit;
                    },
                    syn::parse_quote! {
                        continue #loop_label;
                    },
                ]
            })
        })
    })
}

pub(super) fn current_state_loop_label() -> Option<syn::Lifetime> {
    GOTO_STATE_CONTEXTS.with(|contexts| {
        contexts
            .borrow()
            .last()
            .map(|context| context.loop_label.clone())
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn continue_label_guard_restores_previous_names() {
        assert!(!is_continue_label("retry"));
        {
            let _outer = GotoContinueLabelsGuard::extend(["retry".to_string()]);
            assert!(is_continue_label("retry"));
            {
                let _inner = GotoContinueLabelsGuard::extend(["again".to_string()]);
                assert!(is_continue_label("retry"));
                assert!(is_continue_label("again"));
            }
            assert!(is_continue_label("retry"));
            assert!(!is_continue_label("again"));
        }
        assert!(!is_continue_label("retry"));
    }

    #[test]
    fn state_context_builds_jump_and_restores_loop_label() {
        let state_ident = syn::Ident::new("__gors_state", Span::mixed_site());
        let loop_label = syn::Lifetime::new("'__gors_goto", Span::mixed_site());
        let labels = BTreeMap::from([("Target".to_string(), 3usize)]);

        assert!(compile_state_jump("Target").is_none());
        assert!(current_state_loop_label().is_none());
        {
            let _state =
                GotoStateContextGuard::push(GotoStateContext::new(state_ident, loop_label, labels));
            let jump = compile_state_jump("Target");
            assert!(jump.is_some());
            let jump = jump.unwrap_or_default();
            let current = current_state_loop_label();
            assert!(current.is_some());
            let current = match current {
                Some(current) => current,
                None => syn::Lifetime::new("'__missing", Span::mixed_site()),
            };
            let output = quote!(#(#jump)*).to_string();
            assert!(output.contains("__gors_state = 3usize"));
            assert!(output.contains("continue '__gors_goto"));
            assert_eq!(current.ident, "__gors_goto");
        }
        assert!(compile_state_jump("Target").is_none());
        assert!(current_state_loop_label().is_none());
    }
}
