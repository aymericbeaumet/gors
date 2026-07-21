use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::STDLIB_VERSION;

const MANIFEST_FILENAME: &str = ".gors_manifest.json";
const MANIFEST_VERSION: u32 = 2;
static MANIFEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildManifest {
    pub version: u32,
    #[serde(default)]
    pub compiler_version: String,
    #[serde(default)]
    pub stdlib_version: String,
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
            version: MANIFEST_VERSION,
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            stdlib_version: STDLIB_VERSION.to_string(),
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
        std::fs::create_dir_all(output_dir)?;
        let content = serde_json::to_vec_pretty(self)?;
        let temp_path = loop {
            let suffix = MANIFEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate = output_dir.join(format!(
                "{MANIFEST_FILENAME}.tmp-{}-{suffix}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&candidate)
            {
                Ok(mut file) => {
                    let write_result = (|| -> Result<(), std::io::Error> {
                        file.write_all(&content)?;
                        file.write_all(b"\n")?;
                        file.sync_all()
                    })();
                    if let Err(error) = write_result {
                        drop(file);
                        let _ = std::fs::remove_file(&candidate);
                        return Err(error.into());
                    }
                    break candidate;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        };
        if let Err(error) = std::fs::rename(&temp_path, path) {
            let _ = std::fs::remove_file(temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn needs_recompile(&self, import_path: &str, current_hash: &str) -> bool {
        if self.version != MANIFEST_VERSION
            || self.compiler_version != env!("CARGO_PKG_VERSION")
            || self.stdlib_version != STDLIB_VERSION
        {
            return true;
        }
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
    fn needs_recompile_returns_true_for_old_manifest_metadata() {
        let mut manifest = BuildManifest::new();
        manifest.version = 1;
        manifest.modules.insert(
            "example/foo".to_string(),
            ModuleEntry {
                content_hash: "abc123".to_string(),
                output_file: "example__foo.rs".to_string(),
            },
        );
        assert!(manifest.needs_recompile("example/foo", "abc123"));
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
        assert_eq!(loaded.version, 2);
        assert_eq!(loaded.compiler_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(loaded.stdlib_version, STDLIB_VERSION);
        assert_eq!(loaded.modules.len(), 1);
        assert!(!loaded.needs_recompile("fmt", "aaa"));
    }

    #[test]
    fn saved_generated_output_skips_unchanged_modules() {
        use sha2::Digest;

        let tmp = tempfile::tempdir().unwrap();
        let output = crate::printer::GeneratedOutput {
            files: [
                ("main.rs".to_string(), "fn main() {}\n".to_string()),
                ("alpha.rs".to_string(), "pub fn Alpha() {}\n".to_string()),
            ]
            .into_iter()
            .collect(),
        };

        let mut manifest = BuildManifest::new();
        for (filename, source) in &output.files {
            std::fs::write(tmp.path().join(filename), source).unwrap();
            manifest.modules.insert(
                filename.clone(),
                ModuleEntry {
                    content_hash: sha2::Sha256::digest(source.as_bytes())
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                    output_file: filename.clone(),
                },
            );
        }
        manifest.save(tmp.path()).unwrap();

        let loaded = BuildManifest::load(tmp.path()).unwrap();
        for (filename, source) in &output.files {
            if filename == "lib.rs" || filename == "main.rs" {
                continue;
            }
            let digest = sha2::Sha256::digest(source.as_bytes());
            let hash = digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert!(
                !loaded.needs_recompile(filename, &hash),
                "unchanged module {filename} should not need recompile"
            );
        }

        assert!(loaded.needs_recompile("fmt.rs", "different_hash"));
    }
}
