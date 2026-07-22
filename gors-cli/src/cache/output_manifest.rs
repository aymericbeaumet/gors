use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const FILENAME: &str = ".gors-generated-output.json";
const SCHEMA_VERSION: u32 = 1;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GeneratedOutputManifest {
    schema_version: u32,
    compiler_fingerprint: String,
    stdlib_version: String,
    files: BTreeMap<String, GeneratedFileEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GeneratedFileEntry {
    content_hash: String,
    output_file: String,
}

impl GeneratedOutputManifest {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            compiler_fingerprint: gors::COMPILER_FINGERPRINT.to_string(),
            stdlib_version: gors::STDLIB_VERSION.to_string(),
            files: BTreeMap::new(),
        }
    }

    pub fn load(output_dir: &Path) -> Option<Self> {
        let content = std::fs::read(output_dir.join(FILENAME)).ok()?;
        let manifest: Self = serde_json::from_slice(&content).ok()?;
        manifest.is_compatible().then_some(manifest)
    }

    pub fn save(&self, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(output_dir)?;
        let content = serde_json::to_vec_pretty(self)?;
        let destination = output_dir.join(FILENAME);
        let temporary = loop {
            let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate =
                output_dir.join(format!("{FILENAME}.tmp-{}-{suffix}", std::process::id()));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&candidate)
            {
                Ok(mut file) => {
                    let result = (|| -> Result<(), std::io::Error> {
                        file.write_all(&content)?;
                        file.write_all(b"\n")?;
                        file.sync_all()
                    })();
                    if let Err(error) = result {
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
        if let Err(error) = std::fs::rename(&temporary, destination) {
            let _ = std::fs::remove_file(temporary);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn record(&mut self, filename: String, content_hash: String) {
        self.files.insert(
            filename.clone(),
            GeneratedFileEntry {
                content_hash,
                output_file: filename,
            },
        );
    }

    pub fn matches(&self, filename: &str, content_hash: &str) -> bool {
        self.files.get(filename).is_some_and(|entry| {
            entry.output_file == filename && entry.content_hash == content_hash
        })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[cfg(test)]
    pub fn contains(&self, filename: &str) -> bool {
        self.files.contains_key(filename)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(name, entry)| (name.as_str(), entry.output_file.as_str()))
    }

    fn is_compatible(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.compiler_fingerprint == gors::COMPILER_FINGERPRINT
            && self.stdlib_version == gors::STDLIB_VERSION
    }
}

#[cfg(test)]
mod tests;
