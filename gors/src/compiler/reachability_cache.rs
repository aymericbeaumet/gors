use quote::ToTokens;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
pub(super) struct ReachableItems {
    pub(super) keep: HashSet<usize>,
    pub(super) refs: HashMap<String, HashSet<String>>,
    pub(super) names: HashSet<String>,
}

static REACHABLE_ITEMS_CACHE: OnceLock<Mutex<BTreeMap<String, ReachableItems>>> = OnceLock::new();

fn reachable_items_cache() -> &'static Mutex<BTreeMap<String, ReachableItems>> {
    REACHABLE_ITEMS_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(super) fn cached_items(cache_key: &str) -> Option<ReachableItems> {
    reachable_items_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(cache_key).cloned())
}

pub(super) fn store_items(cache_key: String, entry: &ReachableItems) {
    if let Ok(mut cache) = reachable_items_cache().lock() {
        cache.insert(cache_key, entry.clone());
    }
}

pub(super) struct ReachabilityFingerprint {
    hasher: Sha256,
}

impl ReachabilityFingerprint {
    pub(super) fn new(label: &str) -> Self {
        let mut fingerprint = Self {
            hasher: Sha256::new(),
        };
        fingerprint.part(env!("CARGO_PKG_VERSION").as_bytes());
        fingerprint.part(label.as_bytes());
        fingerprint
    }

    fn part(&mut self, bytes: &[u8]) {
        self.hasher.update((bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
    }

    pub(super) fn push_str(&mut self, value: &str) {
        self.part(value.as_bytes());
    }

    pub(super) fn push_bool(&mut self, value: bool) {
        self.hasher.update([u8::from(value)]);
    }

    pub(super) fn push_len(&mut self, value: usize) {
        self.hasher.update((value as u64).to_le_bytes());
    }

    pub(super) fn push_items(&mut self, items: &[syn::Item]) {
        self.push_len(items.len());
        for item in items {
            self.push_str(&item.to_token_stream().to_string());
        }
    }

    pub(super) fn finish(self) -> String {
        self.hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

pub(super) fn cache_key(
    items: &[syn::Item],
    roots: &HashSet<String>,
    module_names: &HashSet<String>,
) -> String {
    let mut fingerprint = ReachabilityFingerprint::new("reachable-items");
    let mut sorted_roots: Vec<_> = roots.iter().map(String::as_str).collect();
    sorted_roots.sort_unstable();
    fingerprint.push_len(sorted_roots.len());
    for root in sorted_roots {
        fingerprint.push_str(root);
    }
    let mut sorted_modules: Vec<_> = module_names.iter().map(String::as_str).collect();
    sorted_modules.sort_unstable();
    fingerprint.push_len(sorted_modules.len());
    for module_name in sorted_modules {
        fingerprint.push_str(module_name);
    }
    fingerprint.push_items(items);
    fingerprint.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_stable_for_set_order() {
        let items: Vec<syn::Item> = vec![syn::parse_quote! {
            pub fn Needed() {}
        }];
        let roots_a = HashSet::from(["Needed".to_string(), "Other".to_string()]);
        let roots_b = HashSet::from(["Other".to_string(), "Needed".to_string()]);
        let modules_a = HashSet::from(["fmt".to_string(), "io".to_string()]);
        let modules_b = HashSet::from(["io".to_string(), "fmt".to_string()]);

        assert_eq!(
            cache_key(&items, &roots_a, &modules_a),
            cache_key(&items, &roots_b, &modules_b)
        );
    }

    #[test]
    fn cache_key_changes_with_generated_items() {
        let roots = HashSet::from(["Needed".to_string()]);
        let module_names = HashSet::new();
        let needed_items: Vec<syn::Item> = vec![syn::parse_quote! {
            pub fn Needed() {}
        }];
        let other_items: Vec<syn::Item> = vec![syn::parse_quote! {
            pub fn Other() {}
        }];

        assert_ne!(
            cache_key(&needed_items, &roots, &module_names),
            cache_key(&other_items, &roots, &module_names)
        );
    }
}
