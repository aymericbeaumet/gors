use std::collections::{BTreeMap, HashSet};

use super::{ReceiverTypeRef, syn_inspect::named_self_type};

pub(super) type Supertraits = BTreeMap<(String, String), Vec<ReceiverTypeRef>>;

#[derive(Default)]
pub(super) struct Targets<T> {
    by_receiver: BTreeMap<String, T>,
    receiver_keys_by_method: BTreeMap<String, HashSet<String>>,
    by_unambiguous_method: BTreeMap<String, T>,
}

impl<T> Targets<T> {
    pub(super) fn is_empty(&self) -> bool {
        self.by_receiver.is_empty()
    }

    pub(super) fn record_methods_seen(&mut self, module: &str, item_impl: &syn::ItemImpl) {
        let Some(self_name) = named_self_type(&item_impl.self_ty) else {
            return;
        };
        for impl_item in &item_impl.items {
            let syn::ImplItem::Fn(method) = impl_item else {
                continue;
            };
            self.record_method_seen(module, &self_name, &method.sig.ident.to_string());
        }
    }

    pub(super) fn record_method_seen(&mut self, module: &str, self_name: &str, method: &str) {
        self.receiver_keys_by_method
            .entry(method.to_string())
            .or_default()
            .insert(receiver_method_target_key(module, self_name, method));
    }

    pub(super) fn insert_receiver(
        &mut self,
        module: &str,
        self_name: &str,
        method: &str,
        target: T,
    ) -> Option<T> {
        let key = receiver_method_target_key(module, self_name, method);
        self.record_method_seen(module, self_name, method);
        self.by_receiver.insert(key, target)
    }
}

impl<T: Clone> Targets<T> {
    pub(super) fn inherit_supertrait_methods(&mut self, supertraits: &Supertraits) {
        loop {
            let snapshot = self.by_receiver.clone();
            let methods = self
                .receiver_keys_by_method
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let mut changed = false;

            for ((module, self_name), direct_supertraits) in supertraits {
                for supertrait in direct_supertraits {
                    let super_module = supertrait.module.as_deref().unwrap_or(module);
                    for method in &methods {
                        let super_key =
                            receiver_method_target_key(super_module, &supertrait.name, method);
                        let Some(target) = snapshot.get(&super_key) else {
                            continue;
                        };
                        let key = receiver_method_target_key(module, self_name, method);
                        if self.by_receiver.contains_key(&key) {
                            continue;
                        }
                        self.insert_receiver(module, self_name, method, target.clone());
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }
        }
    }

    pub(super) fn finalize_unambiguous_names(&mut self) {
        self.by_unambiguous_method.clear();
        for (method, receiver_keys) in &self.receiver_keys_by_method {
            let Some(receiver_key) = receiver_keys.iter().next() else {
                continue;
            };
            if receiver_keys.len() == 1
                && let Some(target) = self.by_receiver.get(receiver_key)
            {
                self.by_unambiguous_method
                    .insert(method.clone(), target.clone());
            }
        }
    }

    pub(super) fn target_for_call(
        &self,
        current_module: &str,
        method: &str,
        receiver_type: Option<&ReceiverTypeRef>,
    ) -> Option<&T> {
        if let Some(receiver_type) = receiver_type {
            let module = receiver_type.module.as_deref().unwrap_or(current_module);
            let key = receiver_method_target_key(module, &receiver_type.name, method);
            return self.by_receiver.get(&key);
        }
        self.by_unambiguous_method.get(method)
    }
}

fn receiver_method_target_key(module: &str, self_name: &str, method: &str) -> String {
    format!("{module}::{self_name}::{method}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_for_call_uses_receiver_before_method_name_fallback() {
        let mut targets = Targets::default();
        targets.insert_receiver("main", "NeedsMut", "fill", HashSet::from([0_usize]));
        targets.insert_receiver("main", "TakesValue", "fill", HashSet::new());
        targets.finalize_unambiguous_names();

        let needs_mut = ReceiverTypeRef {
            module: Some("main".to_string()),
            name: "NeedsMut".to_string(),
        };
        let takes_value = ReceiverTypeRef {
            module: Some("main".to_string()),
            name: "TakesValue".to_string(),
        };

        assert_eq!(
            targets.target_for_call("main", "fill", Some(&needs_mut)),
            Some(&HashSet::from([0_usize]))
        );
        assert_eq!(
            targets.target_for_call("main", "fill", Some(&takes_value)),
            Some(&HashSet::new())
        );
        assert!(targets.target_for_call("main", "fill", None).is_none());
    }

    #[test]
    fn target_for_call_keeps_unambiguous_method_fallback() {
        let mut targets = Targets::default();
        targets.insert_receiver("main", "Only", "fill", HashSet::from([1_usize]));
        targets.finalize_unambiguous_names();

        assert_eq!(
            targets.target_for_call("main", "fill", None),
            Some(&HashSet::from([1_usize]))
        );
    }
}
