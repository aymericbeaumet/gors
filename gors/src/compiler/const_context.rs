use std::cell::RefCell;
use std::collections::BTreeMap;

use super::{ConstValue, TYPE_ENV, ast, const_eval_expr, typeinfer};

thread_local! {
    static LOCAL_CONST_VALUES: RefCell<Vec<BTreeMap<String, ConstValue>>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct LocalConstScopeGuard;

impl LocalConstScopeGuard {
    pub(super) fn push() -> Self {
        LOCAL_CONST_VALUES.with(|values| values.borrow_mut().push(BTreeMap::new()));
        Self
    }
}

impl Drop for LocalConstScopeGuard {
    fn drop(&mut self) {
        LOCAL_CONST_VALUES.with(|values| {
            values.borrow_mut().pop();
        });
    }
}

pub(super) fn const_eval_expr_in_active_env(
    expr: &ast::Expr,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
) -> Option<ConstValue> {
    let scoped_values = active_values(values);
    TYPE_ENV.with(|env| {
        if let Some(scoped_values) = scoped_values.as_ref() {
            const_eval_expr(expr, iota_value, scoped_values, &env.borrow())
        } else {
            const_eval_expr(expr, iota_value, values, &env.borrow())
        }
    })
}

pub(super) fn set_local_const_value(name: &str, value: ConstValue) {
    LOCAL_CONST_VALUES.with(|values| {
        if let Some(scope) = values.borrow_mut().last_mut() {
            scope.insert(name.to_string(), value);
        }
    });
}

pub(super) fn set_package_const_integer_value(name: &str, value: &ConstValue) {
    let Some(value) = value.as_i128() else {
        return;
    };
    TYPE_ENV.with(|env| env.borrow_mut().set_const_integer_value(name, value));
}

pub(super) fn is_local_const_name(name: &str) -> bool {
    LOCAL_CONST_VALUES.with(|values| {
        values
            .borrow()
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
    })
}

pub(super) fn local_const_go_type_for_expr(expr: &ast::Expr) -> Option<typeinfer::GoType> {
    let ast::Expr::Ident(ident) = expr else {
        return None;
    };
    match local_const_value(ident.name)? {
        ConstValue::Bool(_) => Some(typeinfer::GoType::Bool),
        ConstValue::Complex(_, _) => Some(typeinfer::GoType::Complex128),
        ConstValue::Float(_) => Some(typeinfer::GoType::Float64),
        ConstValue::Int(_) => Some(typeinfer::GoType::Int),
        ConstValue::Rational(_) => Some(typeinfer::GoType::Float64),
        ConstValue::Str(_) => Some(typeinfer::GoType::String),
        ConstValue::Uint(_, _) => Some(typeinfer::GoType::Uint),
    }
}

fn active_values(values: &BTreeMap<String, ConstValue>) -> Option<BTreeMap<String, ConstValue>> {
    LOCAL_CONST_VALUES.with(|local_values| {
        let local_values = local_values.borrow();
        if local_values.is_empty() && values.is_empty() {
            return None;
        }

        let mut merged = BTreeMap::new();
        for scope in local_values.iter() {
            merged.extend(
                scope
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            );
        }
        merged.extend(
            values
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
        Some(merged)
    })
}

fn local_const_value(name: &str) -> Option<ConstValue> {
    LOCAL_CONST_VALUES.with(|values| {
        values
            .borrow()
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_const_scope_restores_previous_values() {
        assert!(!is_local_const_name("n"));
        {
            let _outer = LocalConstScopeGuard::push();
            set_local_const_value("n", ConstValue::Int(1));
            assert!(is_local_const_name("n"));
            assert_eq!(
                local_const_value("n").and_then(|value| value.as_i128()),
                Some(1)
            );
            {
                let _inner = LocalConstScopeGuard::push();
                set_local_const_value("n", ConstValue::Int(2));
                set_local_const_value("m", ConstValue::Int(3));
                assert_eq!(
                    local_const_value("n").and_then(|value| value.as_i128()),
                    Some(2)
                );
                assert!(is_local_const_name("m"));
            }
            assert_eq!(
                local_const_value("n").and_then(|value| value.as_i128()),
                Some(1)
            );
            assert!(!is_local_const_name("m"));
        }
        assert!(!is_local_const_name("n"));
    }

    #[test]
    fn active_values_merge_local_scopes_before_package_values() {
        let package_values = BTreeMap::from([
            ("outer".to_string(), ConstValue::Int(10)),
            ("package_only".to_string(), ConstValue::Int(30)),
        ]);
        let _scope = LocalConstScopeGuard::push();
        set_local_const_value("outer", ConstValue::Int(20));

        let merged = active_values(&package_values);
        assert!(merged.is_some());
        let merged = merged.unwrap_or_default();
        assert_eq!(merged.get("outer").and_then(ConstValue::as_i128), Some(10));
        assert_eq!(
            merged.get("package_only").and_then(ConstValue::as_i128),
            Some(30)
        );
    }
}
