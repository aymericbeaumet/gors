//! Target-neutral identity for generated Rust source products.

use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::Path;

use super::{file_hash, normalized_path};

/// Explicit schema for CLI-owned source selection and compiler invocation.
///
/// CLI implementation details are intentionally not hashed. Bump this only
/// when the driver changes which semantic program is presented to `gors`.
pub const GENERATED_DRIVER_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedRustIdentity {
    fingerprint: String,
}

pub struct GeneratedRustIdentityOptions<'a> {
    pub source_paths: &'a [String],
}

impl GeneratedRustIdentity {
    pub fn new(
        options: GeneratedRustIdentityOptions<'_>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gorspath = std::env::var_os("GORSPATH");
        Self::new_with_facts(
            options,
            gors::GENERATED_RUST_FINGERPRINT,
            GENERATED_DRIVER_SCHEMA,
            gorspath.as_deref(),
        )
    }

    fn new_with_facts(
        options: GeneratedRustIdentityOptions<'_>,
        generated_rust_fingerprint: &str,
        driver_schema: u32,
        gorspath: Option<&OsStr>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hash_part(&mut hasher, b"gors-generated-rust-request-v1");
        hash_part(&mut hasher, &driver_schema.to_le_bytes());
        hash_part(&mut hasher, generated_rust_fingerprint.as_bytes());
        hash_part(&mut hasher, gors::GO_VERSION.as_bytes());
        hash_part(&mut hasher, gors::STDLIB_VERSION.as_bytes());
        hash_part(
            &mut hasher,
            gors_runtime_abi::RuntimeAbiManifest::current()
                .identity()
                .as_bytes(),
        );
        hash_gorspath(&mut hasher, gorspath);

        let mut source_facts = options
            .source_paths
            .iter()
            .map(|source_path| {
                Ok::<_, Box<dyn std::error::Error>>((
                    normalized_path(Path::new(source_path))?,
                    module_context(Path::new(source_path))?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        source_facts.sort();
        for (source_path, module_context) in source_facts {
            hash_part(&mut hasher, source_path.as_bytes());
            hash_part(&mut hasher, module_context.as_bytes());
        }

        Ok(Self {
            fingerprint: hex_digest(hasher.finalize()),
        })
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[cfg(test)]
    pub(super) fn for_test(fingerprint: impl Into<String>) -> Self {
        Self {
            fingerprint: fingerprint.into(),
        }
    }
}

fn module_context(source_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut directory = if source_path.is_dir() {
        source_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    if directory.is_relative() {
        directory = std::env::current_dir()?.join(directory);
    }
    loop {
        let go_mod = directory.join("go.mod");
        if go_mod.is_file() {
            return Ok(format!(
                "{}:{}",
                normalized_path(&go_mod)?,
                file_hash(&go_mod)?
            ));
        }
        if !directory.pop() {
            return Ok("no-go-mod".to_string());
        }
    }
}

fn hash_gorspath(hasher: &mut Sha256, gorspath: Option<&OsStr>) {
    hash_part(hasher, b"gorspath");
    let Some(gorspath) = gorspath else {
        hash_part(hasher, b"unset");
        return;
    };

    hash_part(hasher, b"set");
    hash_os_part(hasher, gorspath);
    for (index, root) in std::env::split_paths(gorspath).enumerate() {
        hash_part(
            hasher,
            &u64::try_from(index).unwrap_or(u64::MAX).to_le_bytes(),
        );
        hash_gorspath_root(hasher, &root);
    }
}

fn hash_gorspath_root(hasher: &mut Sha256, root: &Path) {
    hash_part(hasher, b"root");
    hash_os_part(hasher, root.as_os_str());
    if root.as_os_str().is_empty() {
        hash_part(hasher, b"empty");
        return;
    }

    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(current_dir) => current_dir.join(root),
            Err(error) => {
                hash_io_error(hasher, b"current-directory-error", root, &error);
                return;
            }
        }
    };
    let canonical = match std::fs::canonicalize(&absolute) {
        Ok(canonical) => canonical,
        Err(error) => {
            hash_io_error(hasher, b"missing-or-inaccessible-root", &absolute, &error);
            return;
        }
    };
    hash_part(hasher, b"canonical");
    hash_os_part(hasher, canonical.as_os_str());

    if canonical.is_file() {
        hash_part(hasher, b"file");
    } else if canonical.is_dir() {
        // Exact selected contents and eligible membership come from the one
        // immutable InputSnapshot captured before cache comparison.
        hash_part(hasher, b"directory");
    } else {
        hash_part(hasher, b"unsupported-root-kind");
    }
}

fn hash_io_error(hasher: &mut Sha256, marker: &[u8], path: &Path, error: &std::io::Error) {
    hash_part(hasher, marker);
    hash_os_part(hasher, path.as_os_str());
    hash_part(hasher, format!("{:?}", error.kind()).as_bytes());
}

#[cfg(unix)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::unix::ffi::OsStrExt as _;
    hash_part(hasher, part.as_bytes());
}

#[cfg(windows)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::windows::ffi::OsStrExt as _;
    let bytes = part
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    hash_part(hasher, &bytes);
}

#[cfg(not(any(unix, windows)))]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    hash_part(hasher, part.to_string_lossy().as_bytes());
}

fn hash_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update(part.len().to_le_bytes());
    hasher.update(part);
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn source_fixture() -> (tempfile::TempDir, Vec<String>) {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("main.go");
        std::fs::write(&source, "package main\n").unwrap();
        (directory, vec![source.to_string_lossy().into_owned()])
    }

    #[test]
    fn identity_tracks_generated_schema_and_driver_selection_only() {
        let (_directory, source_paths) = source_fixture();
        let options = || GeneratedRustIdentityOptions {
            source_paths: &source_paths,
        };
        let baseline =
            GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 1, None).unwrap();
        let same = GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 1, None).unwrap();
        let compiler =
            GeneratedRustIdentity::new_with_facts(options(), "compiler-b", 1, None).unwrap();
        let driver =
            GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 2, None).unwrap();

        assert_eq!(baseline, same);
        assert_ne!(baseline, compiler);
        assert_ne!(baseline, driver);
    }

    #[test]
    fn identity_tracks_gorspath_configuration_without_scanning_contents() {
        let (_directory, source_paths) = source_fixture();
        let first_root = tempfile::tempdir().unwrap();
        let package = first_root.path().join("src/example/dependency");
        std::fs::create_dir_all(&package).unwrap();
        let dependency = package.join("dependency.go");
        std::fs::write(&dependency, "package dependency\nconst Value = 1\n").unwrap();
        let first_path = std::env::join_paths([first_root.path()]).unwrap();
        let options = || GeneratedRustIdentityOptions {
            source_paths: &source_paths,
        };

        let baseline = GeneratedRustIdentity::new_with_facts(
            options(),
            "compiler",
            1,
            Some(first_path.as_os_str()),
        )
        .unwrap();
        std::fs::write(&dependency, "package dependency\nconst Value = 2\n").unwrap();
        let content_edit = GeneratedRustIdentity::new_with_facts(
            options(),
            "compiler",
            1,
            Some(first_path.as_os_str()),
        )
        .unwrap();
        assert_eq!(baseline, content_edit);

        let second_root = tempfile::tempdir().unwrap();
        let second_path = std::env::join_paths([second_root.path()]).unwrap();
        let configuration = GeneratedRustIdentity::new_with_facts(
            options(),
            "compiler",
            1,
            Some(second_path.as_os_str()),
        )
        .unwrap();
        assert_ne!(baseline, configuration);
    }

    #[test]
    fn identity_canonicalizes_equivalent_source_spellings() {
        let (directory, source_paths) = source_fixture();
        let alternate = directory
            .path()
            .join(".")
            .join("main.go")
            .to_string_lossy()
            .into_owned();
        let canonical = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &source_paths,
            },
            "compiler",
            1,
            None,
        )
        .unwrap();
        let equivalent = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &[alternate],
            },
            "compiler",
            1,
            None,
        )
        .unwrap();

        assert_eq!(canonical, equivalent);
    }

    #[test]
    fn identity_is_independent_from_source_argument_order() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.go");
        let second = directory.path().join("second.go");
        std::fs::write(&first, "package main\n").unwrap();
        std::fs::write(&second, "package main\n").unwrap();
        let first = first.to_string_lossy().into_owned();
        let second = second.to_string_lossy().into_owned();
        let forward = vec![first.clone(), second.clone()];
        let reverse = vec![second, first];

        let forward = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &forward,
            },
            "compiler",
            1,
            None,
        )
        .unwrap();
        let reverse = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &reverse,
            },
            "compiler",
            1,
            None,
        )
        .unwrap();

        assert_eq!(forward, reverse);
    }
}
