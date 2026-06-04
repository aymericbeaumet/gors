use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub(super) struct RequiredModuleRoots {
    by_module: HashMap<String, HashSet<String>>,
}

impl RequiredModuleRoots {
    pub(super) fn insert_module(&mut self, module: String) {
        self.by_module.entry(module).or_default();
    }

    pub(super) fn merge(&mut self, refs: HashMap<String, HashSet<String>>) -> bool {
        merge_refs(&mut self.by_module, refs)
    }

    pub(super) fn get(&self, module: &str) -> Option<&HashSet<String>> {
        self.by_module.get(module)
    }

    pub(super) fn get_or_empty<'a>(
        &'a self,
        module: &str,
        empty: &'a HashSet<String>,
    ) -> &'a HashSet<String> {
        self.get(module).unwrap_or(empty)
    }

    pub(super) fn is_missing_or_empty(&self, module: &str) -> bool {
        self.get(module).is_none_or(HashSet::is_empty)
    }

    pub(super) fn cloned_or_default(&self, module: &str) -> HashSet<String> {
        self.get(module).cloned().unwrap_or_default()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&String, &HashSet<String>)> {
        self.by_module.iter()
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &String> {
        self.by_module.keys()
    }
}

pub(super) fn merge_refs(
    required: &mut HashMap<String, HashSet<String>>,
    refs: HashMap<String, HashSet<String>>,
) -> bool {
    let mut changed = false;
    for (module, symbols) in refs {
        let entry = required.entry(module).or_default();
        for symbol in symbols {
            changed |= entry.insert(symbol);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_reports_only_new_module_roots_as_changes() {
        let mut required = HashMap::new();

        assert!(merge_refs(
            &mut required,
            HashMap::from([("fmt".to_string(), HashSet::from(["Println".to_string()]))])
        ));
        assert!(!merge_refs(
            &mut required,
            HashMap::from([("fmt".to_string(), HashSet::from(["Println".to_string()]))])
        ));
        assert!(merge_refs(
            &mut required,
            HashMap::from([("fmt".to_string(), HashSet::from(["Stringer".to_string()]))])
        ));
    }

    #[test]
    fn required_roots_distinguish_missing_empty_and_present_modules() {
        let mut required = RequiredModuleRoots::default();

        assert!(required.is_missing_or_empty("fmt"));

        required.insert_module("fmt".to_string());
        assert!(required.is_missing_or_empty("fmt"));

        required.merge(HashMap::from([(
            "fmt".to_string(),
            HashSet::from(["Println".to_string()]),
        )]));
        assert!(!required.is_missing_or_empty("fmt"));
        assert!(required.cloned_or_default("io").is_empty());
    }
}
