use crate::cache::{FileArtifact, GeneratedOutputManifest};
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub struct FileWriteStats {
    pub written: usize,
    pub skipped: usize,
    pub removed: usize,
}

pub struct OutputDirectoryLock {
    _file: std::fs::File,
}

impl OutputDirectoryLock {
    pub fn acquire(output_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(output_dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(output_dir.join(".gors-build.lock"))?;
        file.lock()?;
        Ok(Self { _file: file })
    }

    pub fn spawn_and_release(
        self,
        command: &mut Command,
    ) -> Result<std::process::Child, std::io::Error> {
        let child = command.spawn()?;
        drop(self);
        Ok(child)
    }
}

fn pending_write_priority(path: &Path) -> u8 {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs") => 1,
        Some("main.rs") => 2,
        _ => 0,
    }
}

pub fn prepare_atomic_write(
    path: &Path,
    source: &str,
) -> Result<tempfile::NamedTempFile, Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(source.as_bytes())?;
    temp.as_file_mut().sync_all()?;
    Ok(temp)
}

#[cfg(test)]
pub fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<FileWriteStats, Box<dyn std::error::Error>> {
    let _output_lock = OutputDirectoryLock::acquire(output_dir)?;
    write_generated_output_locked(output, output_dir)
}

pub fn write_generated_output_locked(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<FileWriteStats, Box<dyn std::error::Error>> {
    let previous_manifest = GeneratedOutputManifest::load(output_dir);
    let mut new_manifest = GeneratedOutputManifest::new();
    let mut stats = FileWriteStats {
        written: 0,
        skipped: 0,
        removed: 0,
    };
    let mut pending_writes = Vec::new();

    for (filename, source) in &output.files {
        let file_path = output_dir.join(filename);
        let current_hash = sha2_hash(source);
        let unchanged = previous_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.matches(filename, &current_hash));

        if unchanged && file_path.exists() {
            stats.skipped += 1;
        } else {
            let temp = prepare_atomic_write(&file_path, source)?;
            pending_writes.push((file_path, temp));
            stats.written += 1;
        }

        new_manifest.record(filename.clone(), current_hash);
    }

    // Publish leaf modules before the coordinator files that reference them.
    // Each rename is atomic, while the directory lock prevents concurrent gors
    // builds from interleaving two generated programs in the same directory.
    pending_writes.sort_by_key(|(path, _)| pending_write_priority(path));
    for (file_path, temp) in pending_writes {
        temp.persist(file_path).map_err(|error| error.error)?;
    }

    if let Some(previous_manifest) = &previous_manifest {
        for (filename, output_file) in previous_manifest.files() {
            if output.files.contains_key(filename) {
                continue;
            }
            let file_path = output_dir.join(output_file);
            if file_path.is_file() {
                std::fs::remove_file(&file_path)?;
                stats.removed += 1;
            }
        }
    }

    new_manifest.save(output_dir)?;
    Ok(stats)
}

pub fn write_source_map(
    output: &gors::printer::GeneratedOutput,
    plan: &gors::compiler::SourceMapPlan,
    path: &Path,
) -> Result<FileArtifact, Box<dyn std::error::Error>> {
    let main_source = output
        .files
        .get("main.rs")
        .ok_or("generated program has no main.rs for source-map output")?;
    let source_map = plan.build(main_source);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    source_map.to_writer(temp.as_file_mut())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    FileArtifact::capture(path)
}

fn sha2_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
