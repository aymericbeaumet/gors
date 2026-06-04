use std::cell::RefCell;
use std::collections::HashSet;

use super::{TYPE_ENV, typeinfer};

thread_local! {
    static BYTE_SEQ_TYPE_PARAMS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub(super) struct TypeParamKindsGuard {
    previous: Vec<(String, Option<typeinfer::TypeKind>)>,
}

impl TypeParamKindsGuard {
    pub(super) fn set(type_args: &[syn::Ident]) -> Self {
        Self {
            previous: seed_type_param_kinds(type_args),
        }
    }
}

impl Drop for TypeParamKindsGuard {
    fn drop(&mut self) {
        restore_type_param_kinds(std::mem::take(&mut self.previous));
    }
}

pub(super) struct ByteSeqTypeParamsGuard {
    previous: HashSet<String>,
}

impl ByteSeqTypeParamsGuard {
    pub(super) fn set(current: HashSet<String>) -> Self {
        let previous = BYTE_SEQ_TYPE_PARAMS
            .with(|params| std::mem::replace(&mut *params.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for ByteSeqTypeParamsGuard {
    fn drop(&mut self) {
        BYTE_SEQ_TYPE_PARAMS.with(|params| {
            *params.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) fn is_byte_seq_type_param(go_type: &typeinfer::GoType) -> bool {
    matches!(go_type, typeinfer::GoType::Named(name) if BYTE_SEQ_TYPE_PARAMS.with(|params| params.borrow().contains(name)))
}

fn seed_type_param_kinds(type_args: &[syn::Ident]) -> Vec<(String, Option<typeinfer::TypeKind>)> {
    TYPE_ENV.with(|env| {
        let mut env = env.borrow_mut();
        type_args
            .iter()
            .map(|ident| {
                let name = ident.to_string();
                let previous = env.get_type_kind(&name).cloned();
                env.set_type_kind(&name, typeinfer::TypeKind::TypeParam);
                (name, previous)
            })
            .collect()
    })
}

fn restore_type_param_kinds(previous: Vec<(String, Option<typeinfer::TypeKind>)>) {
    TYPE_ENV.with(|env| {
        let mut env = env.borrow_mut();
        for (name, kind) in previous {
            if let Some(kind) = kind {
                env.set_type_kind(&name, kind);
            } else {
                env.remove_type_kind(&name);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;

    use super::*;

    fn type_kind(name: &str) -> Option<typeinfer::TypeKind> {
        TYPE_ENV.with(|env| env.borrow().get_type_kind(name).cloned())
    }

    #[test]
    fn type_param_kind_guard_restores_previous_kinds() {
        let fresh_name = "__gors_type_param_context_fresh";
        let existing_name = "__gors_type_param_context_existing";
        TYPE_ENV.with(|env| {
            let mut env = env.borrow_mut();
            env.remove_type_kind(fresh_name);
            env.set_type_kind(existing_name, typeinfer::TypeKind::Struct);
        });

        let fresh = syn::Ident::new(fresh_name, Span::mixed_site());
        let existing = syn::Ident::new(existing_name, Span::mixed_site());
        assert_eq!(type_kind(fresh_name), None);
        assert_eq!(type_kind(existing_name), Some(typeinfer::TypeKind::Struct));

        {
            let _guard = TypeParamKindsGuard::set(&[fresh, existing]);
            assert_eq!(type_kind(fresh_name), Some(typeinfer::TypeKind::TypeParam));
            assert_eq!(
                type_kind(existing_name),
                Some(typeinfer::TypeKind::TypeParam)
            );
        }

        assert_eq!(type_kind(fresh_name), None);
        assert_eq!(type_kind(existing_name), Some(typeinfer::TypeKind::Struct));
        TYPE_ENV.with(|env| env.borrow_mut().remove_type_kind(existing_name));
    }

    #[test]
    fn byte_seq_type_params_guard_restores_previous_names() {
        let outer_name = "__gors_byte_seq_outer";
        let inner_name = "__gors_byte_seq_inner";
        let outer = typeinfer::GoType::Named(outer_name.to_string());
        let inner = typeinfer::GoType::Named(inner_name.to_string());
        assert!(!is_byte_seq_type_param(&outer));
        assert!(!is_byte_seq_type_param(&inner));

        {
            let _outer = ByteSeqTypeParamsGuard::set(HashSet::from([outer_name.to_string()]));
            assert!(is_byte_seq_type_param(&outer));
            assert!(!is_byte_seq_type_param(&inner));

            {
                let _inner = ByteSeqTypeParamsGuard::set(HashSet::from([inner_name.to_string()]));
                assert!(!is_byte_seq_type_param(&outer));
                assert!(is_byte_seq_type_param(&inner));
            }

            assert!(is_byte_seq_type_param(&outer));
            assert!(!is_byte_seq_type_param(&inner));
        }

        assert!(!is_byte_seq_type_param(&outer));
        assert!(!is_byte_seq_type_param(&inner));
    }
}
