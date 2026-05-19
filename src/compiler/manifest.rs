use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const MANIFEST_FILENAME: &str = ".gors_manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildManifest {
    pub version: u32,
    pub modules: BTreeMap<String, ModuleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleEntry {
    pub content_hash: String,
    pub output_file: String,
}

impl BuildManifest {
    pub fn new() -> Self {
        Self {
            version: 1,
            modules: BTreeMap::new(),
        }
    }

    pub fn load(output_dir: &Path) -> Option<Self> {
        let path = output_dir.join(MANIFEST_FILENAME);
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let path = output_dir.join(MANIFEST_FILENAME);
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn needs_recompile(&self, import_path: &str, current_hash: &str) -> bool {
        match self.modules.get(import_path) {
            Some(entry) => entry.content_hash != current_hash,
            None => true,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn needs_recompile_returns_true_for_unknown_module() {
        let manifest = BuildManifest::new();
        assert!(manifest.needs_recompile("example/foo", "abc123"));
    }

    #[test]
    fn needs_recompile_returns_false_when_hash_matches() {
        let mut manifest = BuildManifest::new();
        manifest.modules.insert(
            "example/foo".to_string(),
            ModuleEntry {
                content_hash: "abc123".to_string(),
                output_file: "example__foo.rs".to_string(),
            },
        );
        assert!(!manifest.needs_recompile("example/foo", "abc123"));
    }

    #[test]
    fn needs_recompile_returns_true_when_hash_differs() {
        let mut manifest = BuildManifest::new();
        manifest.modules.insert(
            "example/foo".to_string(),
            ModuleEntry {
                content_hash: "abc123".to_string(),
                output_file: "example__foo.rs".to_string(),
            },
        );
        assert!(manifest.needs_recompile("example/foo", "changed"));
    }

    #[test]
    fn round_trip_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = BuildManifest::new();
        manifest.modules.insert(
            "fmt".to_string(),
            ModuleEntry {
                content_hash: "aaa".to_string(),
                output_file: "fmt.rs".to_string(),
            },
        );
        manifest.save(tmp.path()).unwrap();
        let loaded = BuildManifest::load(tmp.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.modules.len(), 1);
        assert!(!loaded.needs_recompile("fmt", "aaa"));
    }
}
