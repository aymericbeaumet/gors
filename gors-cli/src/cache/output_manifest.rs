use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime_descriptor::LINK_OUTPUT_FILENAME;
use crate::runtime_descriptor::RuntimeLinkDescriptor;

const FILENAME: &str = ".gors-generated-output.json";
const SCHEMA_VERSION: u32 = 2;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedOutputManifest {
    schema_version: u32,
    compiler_fingerprint: String,
    stdlib_version: String,
    runtime: RuntimeLinkDescriptor,
    files: BTreeMap<String, GeneratedFileEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GeneratedFileEntry {
    content_hash: String,
    output_file: String,
}

impl GeneratedOutputManifest {
    pub fn new(runtime: RuntimeLinkDescriptor) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            compiler_fingerprint: gors::COMPILER_FINGERPRINT.to_string(),
            stdlib_version: gors::STDLIB_VERSION.to_string(),
            runtime,
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
        if let Err(error) = publish_manifest(&temporary, &destination) {
            let _ = std::fs::remove_file(&temporary);
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

    pub fn runtime(&self) -> &RuntimeLinkDescriptor {
        &self.runtime
    }

    pub fn refresh_runtime(&mut self, runtime: RuntimeLinkDescriptor) {
        self.runtime = runtime;
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
            && self.files.contains_key(LINK_OUTPUT_FILENAME)
            && self.files.iter().all(|(filename, entry)| {
                filename == &entry.output_file
                    && is_normal_relative_filename(filename)
                    && is_sha256(&entry.content_hash)
            })
            && self.runtime.reconstruct_dependency().is_ok()
    }
}

fn publish_manifest(temporary: &Path, destination: &Path) -> Result<(), std::io::Error> {
    // Unix rename replaces atomically. Windows rename cannot replace an
    // existing file; removing this advisory cache record first is fail-safe,
    // because a crash in the gap becomes a cache miss rather than stale reuse.
    #[cfg(windows)]
    match std::fs::remove_file(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::rename(temporary, destination)
}

fn is_normal_relative_filename(filename: &str) -> bool {
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.contains('/')
        || filename.contains('\\')
    {
        return false;
    }
    let mut components = Path::new(filename).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests;
