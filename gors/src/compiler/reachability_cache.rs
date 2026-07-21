use quote::ToTokens;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::CompiledModule;

#[derive(Clone)]
pub(super) struct ReachableItems {
    pub(super) keep: HashSet<usize>,
    pub(super) refs: HashMap<String, HashSet<String>>,
    pub(super) names: HashSet<String>,
}

#[cfg(target_family = "wasm")]
const MAX_REACHABILITY_CACHE_ENTRIES: usize = 128;
#[cfg(not(target_family = "wasm"))]
const MAX_REACHABILITY_CACHE_ENTRIES: usize = 512;
#[cfg(target_family = "wasm")]
const MAX_REACHABILITY_CACHE_BYTES: usize = 16 * 1024 * 1024;
#[cfg(not(target_family = "wasm"))]
const MAX_REACHABILITY_CACHE_BYTES: usize = 64 * 1024 * 1024;

struct CachedReachableItems {
    entry: ReachableItems,
    estimated_bytes: usize,
    last_used: u64,
}

#[derive(Default)]
struct ReachabilityCache {
    entries: BTreeMap<String, CachedReachableItems>,
    estimated_bytes: usize,
    clock: u64,
}

impl ReachabilityCache {
    fn get(&mut self, cache_key: &str) -> Option<ReachableItems> {
        self.clock = self.clock.wrapping_add(1);
        let cached = self.entries.get_mut(cache_key)?;
        cached.last_used = self.clock;
        Some(cached.entry.clone())
    }

    fn insert(
        &mut self,
        cache_key: String,
        entry: &ReachableItems,
        max_entries: usize,
        max_bytes: usize,
    ) {
        let estimated_bytes = cache_key.len() + estimated_reachable_items_bytes(entry);
        if max_entries == 0 || estimated_bytes > max_bytes {
            return;
        }

        if let Some(replaced) = self.entries.remove(&cache_key) {
            self.estimated_bytes = self
                .estimated_bytes
                .saturating_sub(replaced.estimated_bytes);
        }
        self.clock = self.clock.wrapping_add(1);
        self.estimated_bytes = self.estimated_bytes.saturating_add(estimated_bytes);
        self.entries.insert(
            cache_key,
            CachedReachableItems {
                entry: entry.clone(),
                estimated_bytes,
                last_used: self.clock,
            },
        );

        while self.entries.len() > max_entries || self.estimated_bytes > max_bytes {
            let Some(eviction_key) = self
                .entries
                .iter()
                .min_by_key(|(key, cached)| (cached.last_used, *key))
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&eviction_key) {
                self.estimated_bytes = self.estimated_bytes.saturating_sub(evicted.estimated_bytes);
            }
        }
    }
}

fn estimated_reachable_items_bytes(entry: &ReachableItems) -> usize {
    let keep = entry
        .keep
        .len()
        .saturating_mul(std::mem::size_of::<usize>());
    let names = entry.names.iter().map(String::len).sum::<usize>();
    let refs = entry
        .refs
        .iter()
        .map(|(module, roots)| module.len() + roots.iter().map(String::len).sum::<usize>())
        .sum::<usize>();
    keep.saturating_add(names).saturating_add(refs)
}

static REACHABLE_ITEMS_CACHE: OnceLock<Mutex<ReachabilityCache>> = OnceLock::new();

fn reachable_items_cache() -> &'static Mutex<ReachabilityCache> {
    REACHABLE_ITEMS_CACHE.get_or_init(|| Mutex::new(ReachabilityCache::default()))
}

pub(super) fn cached_items(cache_key: &str) -> Option<ReachableItems> {
    reachable_items_cache()
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(cache_key))
}

pub(super) fn store_items(cache_key: String, entry: &ReachableItems) {
    if let Ok(mut cache) = reachable_items_cache().lock() {
        cache.insert(
            cache_key,
            entry,
            MAX_REACHABILITY_CACHE_ENTRIES,
            MAX_REACHABILITY_CACHE_BYTES,
        );
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

#[cfg(test)]
pub(super) fn cache_key(
    items: &[syn::Item],
    roots: &HashSet<String>,
    module_names: &HashSet<String>,
) -> String {
    let items_fingerprint = items_fingerprint(items);
    cache_key_with_items_fingerprint(&items_fingerprint, roots, module_names)
}

pub(super) fn items_fingerprint(items: &[syn::Item]) -> String {
    let mut fingerprint = ReachabilityFingerprint::new("items");
    fingerprint.push_items(items);
    fingerprint.finish()
}

pub(super) fn cache_key_with_items_fingerprint(
    items_fingerprint: &str,
    roots: &HashSet<String>,
    module_names: &HashSet<String>,
) -> String {
    let mut fingerprint = ReachabilityFingerprint::new("reachable-items");
    fingerprint.push_str(items_fingerprint);
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
    fingerprint.finish()
}

pub(super) fn modules_fingerprint(modules: &BTreeMap<String, CompiledModule>) -> String {
    let mut fingerprint = ReachabilityFingerprint::new("modules");
    fingerprint.push_len(modules.len());
    for (key, module) in modules {
        fingerprint.push_str(key);
        fingerprint.push_str(&module.mod_name);
        fingerprint.push_str(&module.import_path);
        fingerprint.push_str(&module.filename);
        fingerprint.push_bool(module.is_main);
        fingerprint.push_bool(module.is_stdlib);
        fingerprint.push_items(&module.file.items);
    }
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
        let items_fingerprint = items_fingerprint(&items);
        assert_eq!(
            cache_key(&items, &roots_a, &modules_a),
            cache_key_with_items_fingerprint(&items_fingerprint, &roots_a, &modules_a)
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

    #[test]
    fn cache_evicts_least_recently_used_entries_at_the_bound() {
        fn entry(name: &str) -> ReachableItems {
            ReachableItems {
                keep: HashSet::from([0]),
                refs: HashMap::new(),
                names: HashSet::from([name.to_string()]),
            }
        }

        let mut cache = ReachabilityCache::default();
        cache.insert("a".to_string(), &entry("a"), 2, usize::MAX);
        cache.insert("b".to_string(), &entry("b"), 2, usize::MAX);
        assert!(cache.get("a").is_some());
        cache.insert("c".to_string(), &entry("c"), 2, usize::MAX);

        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_none());
        assert!(cache.get("c").is_some());
    }
}
